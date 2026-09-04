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

Interfaces never provide an implementation for anything — that is what makes
a member an obligation, not a per-member choice inside the block. So a
required member is written with the exact same keyword a class would use —
``def``, ``classmethod``, ``getter``, ``setter``, ``factory`` — with no body,
and no separate marker keyword is needed to say so, because every member in
an interface is already bodyless by virtue of being in one:

.. code-block:: python

   interface Sized:
       def __len__(self) -> int

   interface Cache[=K, =V]:
       def get(self, key: K) -> V | none
       def put(self, key: K, value: V) -> none
       def is_fresh(self, key: K) -> bool

A classmethod obligation reads the same way:

.. code-block:: python

   interface Buildable:
       classmethod from_default(cls) -> Self

A body is what turns a member into an implementation rather than a
requirement, so writing one inside an interface is an error. That gets
Python's ``@abstractmethod`` check for free, without needing a decorator or
a separate marker keyword at all.

Field, getter, and setter obligations compose
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

A field, a getter, and a setter obligation are each a claim about one
capability — read access, write access, or both — and satisfaction follows
the same width-subtyping already used for mutable, read-only, and immutable
views (`Mutable, read-only, and immutable views <types.rst>`__): whatever
provides at least the capability asked for satisfies the obligation, the
same way a mutable ``T`` is usable wherever the narrower ``&T`` is expected.

A stored field provides both read and write access, so it satisfies a
``getter``-only obligation, a ``setter``-only obligation, or a plain field
obligation, on its own:

.. code-block:: python

   interface Readable:
       getter x(self) -> float

   class Point:
       x: float   # satisfies Readable: reading a field needs no write access

A ``getter`` and ``setter`` pair together provide exactly what a plain field
does — read and write, nothing more — so the pair satisfies a plain field
obligation the same way a field satisfies the pair:

.. code-block:: python

   interface Located:
       x: float

   class ComputedPoint:
       getter x(self) -> float:
           return self._x

       setter x(self, value: float):
           self._x = value

A ``getter`` alone does not satisfy a ``setter`` obligation, or the reverse:
read and write are independent capabilities, and neither implies the other.

A ``final`` field obligation does not compose the way plain field, getter,
and setter obligations did above — it asks for something stronger. ``final``
only promises that the binding is never rebound to a different object; it
says nothing about that object's own contents (see `Final fields`_). A
``getter`` does not satisfy it, even alone with no setter:

.. code-block:: python

   interface HasModel:
       final model: InferenceModel

   class FixedModel:
       final model: InferenceModel   # satisfies HasModel

   class RebuiltModel:
       getter model(self) -> InferenceModel:   # does not satisfy HasModel
           return InferenceModel(self.weights, self.metadata, {:})

A getter's signature only promises no direct external write — nothing about
it rules out ``RebuiltModel`` handing back a different object on every
call, which is exactly what ``final`` rules out. A ``getter`` and ``setter``
pair satisfies it even less: a setter is an explicit write path, and
``final`` rules out any write path existing at all. There is currently no
way to mark a getter as provably returning the same object every call, so a
``final`` field obligation can only be satisfied by a ``final`` field.

Every class is checked for unimplemented obligations before it can be
constructed. A class with any remaining bodyless member from an interface,
trait, or parent class is abstract for construction purposes, matching
Python's useful abstract-class instantiation check without using decorators.
Lucid gets that check without requiring classes to inherit from ``ABC`` or
take on ``ABCMeta`` as a metaclass.

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
class.

The same interface in Lucid is just the required member:

.. code-block:: python

   interface Sized:
       def __len__(self) -> int

No structural interfaces
~~~~~~~~~~~~~~~~~~~~~~~~~~

Lucid interfaces are nominal. A class satisfies an interface only by
explicitly listing it in the class header — ``class Foo(SomeInterface)`` —
never merely by happening to define members with matching names and types.
This holds even for the numeric capability interfaces such as
``SupportsInt``: ``int`` satisfies them because it explicitly inherits from
them, not because it happens to define ``__int__``.

Python's ``typing.Protocol`` takes the opposite approach: any object with
matching methods satisfies a ``Protocol``, whether or not its author ever
intended to make that promise. That costs something real, not just style.

An interface is a promise about behavior, not a description of a method
table, and having a matching method is not the same as making that promise.
Two unrelated methods can share a name and signature by coincidence without
sharing a meaning: ``close(self) -> none`` on a file handle releases a
resource; ``close(self) -> none`` on a negotiation finalizes a deal.
Structural matching only ever compares shape, never intent, so a
``Closeable``-shaped structural interface would silently accept both, and
code written for one meaning could be handed the other with no error at all.
Only the author of a class can say which promise, if any, a method is
actually making — which is exactly what listing the interface by name in
the class header does and mere structural matching cannot.

Nominal declaration is also cheaper to check. Satisfying an interface is
verified once, at the class's own definition, against the fixed, usually
short list of interfaces that class actually names. Structural satisfaction
has no such fixed point: it would have to be rechecked at every place a
value meets an interface-typed slot, comparing that slot's full member
signatures against whatever value happens to show up there, for as long as
either side might change.

Implementing an interface after the fact
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Nominal satisfaction has a real cost of its own: it normally means writing
``class Foo(SomeInterface)`` at ``Foo``'s own definition, which is not
possible when ``Foo`` is a builtin or a third-party type you do not control.
Sometimes an existing type should satisfy an interface its own author never
wrote it against.

``implement`` attaches an implementation to a type from outside that type's own
definition:

.. code-block:: python

   implement Sized for ThirdPartyBuffer:
       def __len__(self) -> int:
           return self.byte_count

``ThirdPartyBuffer`` did not declare ``Sized`` when it was defined; this
``implement`` block is what makes it satisfy that interface, checked the same way
any other implementation is — does this block actually provide everything
``Sized`` declares.

``implement`` is not unrestricted. It is valid only if its author owns the
interface or the type — never neither. That rule, not a runtime check, is
what keeps two unrelated packages from each attaching a different
``implement Sized for ThirdPartyBuffer`` and leaving which one wins to import
order: for any interface/type pair, at most one project in the whole
dependency graph is ever eligible to write it, because ownership of each
name is unique. The members an ``implement`` block adds are also only visible
where the interface itself is imported, not ambiently on every
``ThirdPartyBuffer`` everywhere — the same reasoning that removed ``global``
and ``nonlocal``: nothing about a value's usable surface should change
because of something declared elsewhere that the reader never imported.

Subclassing cannot substitute for this. ``class Buf(ThirdPartyBuffer,
Sized): ...`` only covers instances constructed through ``Buf`` — it does
nothing for a buffer that already exists, built by code that is never going
to construct your subclass, which is the case this problem actually shows
up in. It does not compose, either: two independent subclasses of
``ThirdPartyBuffer``, each adding a different capability, cannot later be
combined into one class, because that class would need two class parents,
which class inheritance does not allow (`One class parent`_). ``implement``
has neither limit — it changes what the original type itself satisfies, for
every instance, and any number of unrelated ``implement`` blocks for the
same type coexist without ever needing to be reconciled into one class.

Python's ``typing.Protocol`` solves the same retrofitting problem
differently: any type with matching methods satisfies a ``Protocol``
automatically, without its author writing anything at all. That is exactly
the accidental-collision risk described above — a type can match a
``Protocol``'s shape by coincidence, without its author ever intending to
make that promise, and Python has no way to tell an intentional
implementation from a lucky guess. ``implement`` gets structural typing's actual
benefit — an existing type outside your control can come to satisfy an
interface — without that risk. It is still an explicit, checked declaration,
not a shape match: someone has to write it, the checker verifies it actually
provides what the interface declares, and the ownership rule guarantees
that whoever wrote it was entitled to make that claim about that type.
Retrofitting is possible; accidental retrofitting is not.

Higher-kinded interfaces
~~~~~~~~~~~~~~~~~~~~~~~~~~

An interface can be generic over a type constructor instead of an ordinary
type, using ``F[_]`` (see
`Higher-kinded parameters <types.rst>`_). Inside such an interface, ``Self``
takes its kind from how the interface's own members use it: subscripting
``Self`` with a type parameter the member declares itself, rather than one
of the class's own, is what makes ``Self`` behave as a constructor rather
than an already-saturated type. No separate marker is needed on ``Self``
the way one is needed on a free-standing parameter like ``F`` — the
interface's member list is already the explicit, checked-once contract that
a free-standing generic parameter does not have.

.. code-block:: python

   interface Functor:
       classmethod map[A, B](cls, tree: Self[A], f: Callable[[A], B]) -> Self[B]

``list`` was not defined with ``Functor``, so it satisfies it through
``implement``:

.. code-block:: python

   implement Functor for list:
       classmethod map[A, B](cls, tree: Self[A], f: Callable[[A], B]) -> Self[B]:
           return [f(x) for x in tree]

Bounding a free-standing generic parameter by a higher-kinded interface
gets one signature checked once for every implementer, present and future,
instead of the open set of independently-checked cases the dispatch-based
``tree_map`` in `Multiple dispatch <dispatch.rst>`_ uses:

.. code-block:: python

   def tree_map[F[_]: Functor, A, B](tree: F[A], f: Callable[[A], B]) -> F[B]:
       return F.map(tree, f)

Which to reach for is the same question either way: dispatch is enough when
each container's traversal only needs to be locally correct on its own; a
bounded, higher-kinded parameter earns its cost when every implementer,
including ones that do not exist yet, needs to be provably checked against
the same signature.

Traits
------

Traits provide reusable behavior. A trait can depend on interface obligations
and provide method bodies in terms of those obligations, but it does not own
stored state or concrete identity.

Traits provide reusable method bodies. They do not declare fields.

.. code-block:: python

   interface Renderable:
       def render(self) -> str

   trait DebugRenderable(Renderable):
       def debug(self) -> str:
           return "<debug " + self.render() + ">"

Trait conflicts are explicit. If two traits define the same method, the class
must resolve the collision.

Multiple inheritance in Python
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Python multiple inheritance uses the same base-class list for several different
jobs: *class inheritance*, interface promises, mixin behavior, metaclass
selection, and MRO construction. Those jobs interfere with each other.

Common pitfalls include:

* MRO order changes which implementation a method call reaches
* cooperative ``super()`` only works when every class in the chain follows the
  same calling convention
* base classes can bring incompatible constructor requirements

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

Base classes can require incompatible initialization protocols.

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
initialize the other base.

Lucid avoids those pitfalls by separating the roles. A class may use
*class inheritance* — inheriting from at most one other class — because a
class is the only one of the three roles that owns stored data. It can
satisfy any number of interfaces because interfaces only declare
obligations. It can use any number of traits because traits provide
reusable bodies without owning state or identity. If traits collide, the
class must resolve the conflict explicitly.

Explicit overrides
~~~~~~~~~~~~~~~~~~~~

A method that replaces an inherited implementation — the class's one class
parent's, or a trait's — must be marked ``override``:

.. code-block:: python

   class Timestamped:
       def save(self):
           self.updated_at = now()

   class Document(Timestamped):
       override def save(self):
           super().save()
           write_to_disk(self)

The same rule governs ``override`` as governs variance markers: the checker
warns and offers an autofix while drafting, but the marker must be written
into the source before the API is accepted. That protects against both
directions of the same mistake — a parent or trait gaining a method that
silently starts shadowing an unrelated method of the same name with no
signal anywhere, and an intended override whose name or signature no longer
matches anything, silently becoming an unrelated new method while the
original goes on being called elsewhere.

Because a class has at most one class parent, calling through to the
overridden implementation is unambiguous — ``super()`` always means that one
parent, never a position in an MRO. A linter checks that an ``override``
method calls it, but this check is a suggestion, not a rule: some overrides
exist specifically to replace an inherited implementation entirely, such as
a class resolving a conflict between two traits, and the warning can be
suppressed for those.

Final methods
~~~~~~~~~~~~~~~

``final`` applies to a method the same way it applies to a field or a
class: the same keyword, one relationship fixed permanently — here, that
the method can be overridden at all.

.. code-block:: python

   class Timestamped:
       final def save(self):
           self.updated_at = now()

   class Document(Timestamped):
       override def save(self):  # error: save is final
           ...

``final`` is not valid on an interface member. It protects a method's
implementation from being replaced, and an interface member has no
implementation to protect — there is nothing there yet for ``final`` to
fix in place.

Classes
-------

Classes define concrete state, construction, and identity. A class can
satisfy interfaces, use traits, and use *class inheritance* to extend at
most one other class, but stored fields and construction belong to the
class.

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

Read-only methods with ``&Self``
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

``Self`` is a builtin type, referring to the enclosing class. A method's
``self`` parameter is ``Self`` by default — the ordinary mutable type — but
a method that only reads its object, never writing to it, should annotate
it ``self: &Self``, the read-only view (see
`Mutable, read-only, and immutable views <types.rst>`_):

.. code-block:: python

   class Counter:
       value: int

       def get(self: &Self) -> int:
           return self.value

       def increment(self):
           self.value += 1

This is what lets a frozen object know which of its methods are safe to
call. Since both ``Self`` and ``!Self`` are subtypes of ``&Self``, a method
declared ``self: &Self`` is callable on a mutable, read-only, or frozen
receiver, while a method left at the default ``self: Self`` requires a
mutable receiver and is not available on a read-only or frozen value at
all:

.. code-block:: python

   counter: Counter = Counter(0)
   frozen: !Counter = freeze(counter)

   frozen.get()        # fine: get takes self: &Self
   frozen.increment()  # error: increment takes self: Self, frozen is not mutable

A ``getter`` is read-only by construction — computing a value from an
object should never require write access — so it behaves as though
``self: &Self`` were already implied.

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

Final fields
^^^^^^^^^^^^

``final`` marks a field that can be set once and never reassigned again,
regardless of which view — ``T``, ``&T``, or ``!T`` — the caller holds. It
is a property of the binding, not the type: ``!T`` says the value on the
other end of a reference cannot change; ``final`` says the reference itself
cannot be pointed somewhere else. The two compose independently:

.. code-block:: python

   class Session:
       final id: str
       final model: InferenceModel
       final config: !InferenceModel

``id`` and ``model`` cannot be rebound after construction, but ``model``'s
own fields can still be mutated in place, since ``InferenceModel`` on its own
is an ordinary mutable type. ``config`` cannot be rebound, and the object it
points to cannot change either.

A ``final`` field is normally set once, in a ``factory``. Assigning to it
again anywhere afterward — from any method, through any view — is an error.

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
not a class option; it is represented by the ``!T`` view.

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

*Class inheritance* stays explicit and does not use Python's metaclass or MRO
customization hooks.

One class parent
^^^^^^^^^^^^^^^^^

*Class inheritance* is limited to one parent: a class may have at most one
class parent. A class header can combine one class parent, any number of
interfaces, and any number of traits.

.. code-block:: python

   class FileLogger(LoggerBase, Closeable, Timestamped):
       path: str

Final classes
^^^^^^^^^^^^^

``final`` applies to a class the same way it applies to a field: a
relationship is fixed permanently, no matter who is asking. On a field, that
relationship is the binding; on a class, it is class inheritance. A
``final`` class may not be used as anyone's class parent:

.. code-block:: python

   final class Point:
       x: float
       y: float

   class Point3D(Point):  # error: Point is final
       z: float

This lets an author close off a class specifically because a subclass could
violate an invariant the implementation depends on, without that decision
touching anything about fields or ordinary variable bindings — the same
``final`` keyword, applied one level up.

No private-name mangling
^^^^^^^^^^^^^^^^^^^^^^^^

Python rewrites a name such as ``self.__a`` to include the defining class's
name. This name mangling helps prevent accidental collisions between members
introduced by different classes, especially in multiple-inheritance
hierarchies.

A class can have at most one class parent. The restriction is not about the
parent being non-abstract — it is that a class, unlike an interface or a
trait, owns stored data, and combining stored data from more than one parent
is exactly what produces the collisions above. Interfaces do not provide
storage, traits do not own stored state, and trait member conflicts must be
resolved explicitly. Because class shape is declared and inherited member
collisions can therefore be reported directly, Lucid does not need automatic
name mangling. Declare and use ``a`` as ``self.a``; ``self.__a`` is not a
special spelling for private state.

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
       def get(self, key: K) -> V | none
       def put(self, key: K, value: V) -> none
       def is_fresh(self, key: K) -> bool

   interface Sized:
       def __len__(self) -> int

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
