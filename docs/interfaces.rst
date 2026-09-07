Interfaces
==========

.. contents:: Table of contents
   :depth: 2
   :local:

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
-----------------------------------------------

A field, a getter, and a setter obligation are each a claim about one
capability — read access, write access, or both — and satisfaction follows
the same width-subtyping already used for mutable, read-only, and immutable
views (`Mutable, read-only, and immutable views <mutability.rst>`__): whatever
provides at least the capability asked for satisfies the obligation, the
same way a mutable ``T`` is usable wherever the narrower ``~T`` is expected.

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
says nothing about that object's own contents (see `Final fields <classes.rst>`_). A
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
--------------------

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
--------------------------

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
------------------------------------------

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
which class inheritance does not allow (`One class parent <classes.rst>`_). ``implement``
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
--------------------------

An interface can be generic over a type constructor instead of an ordinary
type, using ``F[_]`` (see
`Higher-kinded parameters <generics.rst>`_). Inside such an interface, ``Self``
takes its kind from how the interface's own members use it: subscripting
``Self`` with a type parameter the member declares itself, rather than one
of the class's own, is what makes ``Self`` behave as a constructor rather
than an already-saturated type. No separate marker is needed on ``Self``
the way one is needed on a free-standing parameter like ``F`` — the
interface's member list is already the explicit, checked-once contract that
a free-standing generic parameter does not have.

.. code-block:: python

   interface Functor:
       classmethod map[A, B](cls, tree: Self[A], f: (A) -> B) -> Self[B]

``list`` was not defined with ``Functor``, so it satisfies it through
``implement``:

.. code-block:: python

   implement Functor for list:
       classmethod map[A, B](cls, tree: Self[A], f: (A) -> B) -> Self[B]:
           return [f(x) for x in tree]

Bounding a free-standing generic parameter by a higher-kinded interface
gets one signature checked once for every implementer, present and future,
instead of the open set of independently-checked cases the dispatch-based
``tree_map`` in `Multiple dispatch <dispatch.rst>`_ uses:

.. code-block:: python

   def tree_map[F[_]: Functor, A, B](tree: F[A], f: (A) -> B) -> F[B]:
       return F.map(tree, f)

Which to reach for is the same question either way: dispatch is enough when
each container's traversal only needs to be locally correct on its own; a
bounded, higher-kinded parameter earns its cost when every implementer,
including ones that do not exist yet, needs to be provably checked against
the same signature.

