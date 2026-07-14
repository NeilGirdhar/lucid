Modern type specification
=========================

Lucid modernizes how user-defined types are specified. Python spreads this work
across classes, dataclasses, ABCs, protocols, mixins, descriptors, properties,
constructors, metaclasses, and special methods. Lucid replaces that scattered
model with three kinds of type specification: interfaces, traits, and classes.

.. contents:: Table of contents
   :depth: 3
   :local:

Lucid separates user-defined type specification into three kinds.

Definitions are private by default. Public type definitions are marked with
``export`` at the definition or re-export site.

The sections below explain these three kinds, why Python mixes them together,
and how Lucid keeps them separate.

Interfaces
----------

Interfaces specify obligations. They say which fields, methods, getters,
setters, class methods, or factories a type must provide, but they do not store
data and do not provide method bodies.

Interfaces declare required APIs with ``declare``:

.. code-block:: python

   interface Sized:
       declare __len__(self) -> int

   interface Cache[=K, =V]:
       declare get(self, key: K) -> V | none
       declare put(self, key: K, value: V) -> none
       declare is_fresh(self, key: K) -> bool

Interface members use ``declare``, not ``def``, because they specify a callable
requirement without implementing it. ``declare`` is also the abstraction marker:
Lucid does not need Python's ``@abstractmethod`` decorator.

Every class is checked for unimplemented declared obligations before it can be
constructed. A class with any remaining ``declare`` member from an interface,
trait, or parent class is abstract for construction purposes, matching Python's
useful abstract-class instantiation check without using decorators. Lucid gets
that check without requiring classes to inherit from ``ABC`` or take on
``ABCMeta`` as a metaclass.

Interfaces in Python
~~~~~~~~~~~~~~~~~~~~

In Python, an interface is specified as follows:

.. code-block:: python

   class SizedABC(ABC):
       @abstractmethod
       def __len__(self) -> int:
           raise NotImplementedError

This is wordy for a callable requirement. It also imposes ``ABCMeta`` on child
classes, which can limit which metaclasses those children can use.

.. code-block:: python

   class ModelMeta(type):
       ...

   class ModelBase(metaclass=ModelMeta):
       ...

   class SizedABC(ABC):
       @abstractmethod
       def __len__(self) -> int:
           raise NotImplementedError

   class SizedModel(ModelBase, SizedABC):
       def __len__(self) -> int:
           return 0

This class definition fails unless ``ModelMeta`` is made compatible with
``ABCMeta``. The interface requirement has forced a metaclass choice onto the
concrete class.

The same interface in Lucid is just the required member:

.. code-block:: python

   interface Sized:
       declare __len__(self) -> int

Traits
------

Traits provide reusable behavior. A trait can depend on interface obligations
and provide method bodies in terms of those obligations, but it does not own
stored state or concrete identity.

Traits provide reusable method bodies. They do not declare fields.

.. code-block:: python

   interface Renderable:
       declare render(self) -> str

   trait DebugRenderable(Renderable):
       def debug(self) -> str:
           return "<debug " + self.render() + ">"

Trait conflicts are explicit. If two traits define the same method, the class
must resolve the collision.

Multiple inheritance in Python
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Python multiple inheritance uses the same base-class list for several different
jobs: concrete inheritance, interface promises, mixin behavior, metaclass
selection, and MRO construction. Those jobs interfere with each other.

Common pitfalls include:

* MRO order changes which implementation a method call reaches
* cooperative ``super()`` only works when every class in the chain follows the
  same calling convention
* concrete base classes can bring incompatible constructor requirements

Cooperative ``super()``
^^^^^^^^^^^^^^^^^^^^^^^

Cooperative multiple inheritance requires every class in the chain to accept
and forward compatible arguments. One class that does not participate breaks
the chain.

.. code-block:: python

   class Audited:
       def save(self, *, audit: bool = True):
           if audit:
               record_audit()
           return super().save(audit=audit)

   class Timestamped:
       def save(self):
           self.updated_at = now()
           return super().save()

   class Document(Audited, Timestamped, BaseDocument):
       pass

   Document().save()

``Audited.save`` forwards ``audit`` to ``Timestamped.save``, but
``Timestamped.save`` does not accept that keyword. The method chain only works
when every participant follows the same forwarding convention.

Incompatible constructors
^^^^^^^^^^^^^^^^^^^^^^^^^

Concrete base classes can require incompatible initialization protocols.

.. code-block:: python

   class FileBacked:
       def __init__(self, path: Path):
           self.path = path

   class NetworkBacked:
       def __init__(self, host: str, port: int):
           self.host = host
           self.port = port

   class Cache(FileBacked, NetworkBacked):
       pass

There is no obvious generated constructor for ``Cache``. One parent needs a
path, the other needs host and port, and neither constructor explains how to
initialize the other concrete base.

Lucid avoids those pitfalls by separating the roles. A class may have at most
one concrete parent. It can satisfy any number of interfaces because interfaces
only declare obligations. It can use any number of traits because traits provide
reusable bodies without owning state or concrete identity. If traits collide,
the class must resolve the conflict explicitly.

Classes
-------

Classes define concrete state, construction, and identity. A class can satisfy
interfaces, use traits, and inherit from one concrete parent, but stored fields
and construction belong to the class.

.. code-block:: python

   class Point:
       x: float
       y: float

       factory __init__(cls, x: float, y: float):
           return construct(x, y)

       getter distance_from_origin(self) -> float:
           return sqrt(self.x ** 2 + self.y ** 2)

Factories construct fully initialized instances of the exact class where the
factory is defined. Construction is not split across allocation, mutation, and
post-initialization hooks.

Classes own data and concrete behavior. Interfaces declare obligations. Traits
provide reusable method bodies. Trait conflicts are resolved explicitly by the
class.

Object shape and attribute access
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Classes have visible state
^^^^^^^^^^^^^^^^^^^^^^^^^^

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
^^^^^^^^^^^^^^^^^^^^

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

       def get(self, name: str) -> object | none:
           return self.fields.get(name)

       def set(self, name: str, value: object):
           self.fields[name] = value

No descriptors
^^^^^^^^^^^^^^

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
^^^^^^^^^^^^^^

Lucid does not include ``__del__`` finalizers. Cleanup should be explicit in the
API that owns the resource, instead of being hidden behind object destruction
timing.

Class members and construction
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Closed member kinds
^^^^^^^^^^^^^^^^^^^

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
^^^^^^^^^^^^^^^^^^^^^^

Class member variables are marked with ``classvar``. A plain annotated
assignment in the class body declares an instance field; if it has a value, that
value is the field's default.

.. code-block:: python

   class User:
       classvar count: int = 0
       name: str
       active: bool = true

This separates shared class state from stored instance fields.

No static methods for namespaced functions
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

If a function does not need access to an object or class, it stays a function.
Lucid does not use static methods as namespaced free functions.

Explicit class methods
^^^^^^^^^^^^^^^^^^^^^^

Class methods receive ``cls`` and are inherited as class-level behavior.

.. code-block:: python

   class User:
       classvar count: int = 0

       classmethod total_created(cls) -> int:
           return cls.count

Factory construction
^^^^^^^^^^^^^^^^^^^^

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
^^^^^^^^^^^^^^^^^^^^^^^

Classes can request common value semantics with class options. Immutability is
not a class option; it is represented by the ``T!`` view.

.. code-block:: python

   class Point(eq=true, order=true, hash=true):
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

Interfaces, traits, and inheritance
-----------------------------------

Interfaces, traits, and classes work together when a small required core can
support rich reusable behavior. A cache only has to say how to fetch, store, and
report freshness; traits can build higher-level behavior from those obligations:

.. code-block:: python

   interface Cache[=K, =V]:
       declare get(self, key: K) -> V | none
       declare put(self, key: K, value: V) -> none
       declare is_fresh(self, key: K) -> bool

   interface Sized:
       declare __len__(self) -> int

   trait CacheLookup[K, V](Cache[K, V]):
       def get_or_put(self, key: K, build: Callable[[], V]) -> V:
           cached = self.get(key)
           if cached is not none and self.is_fresh(key):
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

       def get(self, key: K) -> V | none:
           return self.entries.get(key)

       def put(self, key: K, value: V) -> none:
           self.entries[key] = value
           self.fresh.add(key)

       def is_fresh(self, key: K) -> bool:
           return key in self.fresh

       def __len__(self) -> int:
           return len(self.entries)

   cache = MemoryCache[str, User]()
   user = cache.get_or_put("ada", lambda: load_user("ada"))
