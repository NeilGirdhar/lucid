# Traits

A trait specifies obligations, provides reusable behavior, or both at
once. It says which fields, methods, getters, setters, class methods,
or factories a type must provide, and it can supply a body for any of
them — but it does not store data or own concrete identity.

## Body or no body

A member with no body is an obligation: something a class using the
trait must provide. A member with a body is a default: reusable
behavior every user of the trait gets for free, in terms of whatever
else the trait requires. The two live in the same declaration, mixed
freely — nothing marks the difference except whether a body follows:

```python
trait Sized:
    def __len__(self) -> int

trait Cache[=K, =V]:
    def get(self, key: K) -> V | none
    def put(self, key: K, value: V) -> none
    def is_fresh(self, key: K) -> bool

    def get_or_put(self, key: K, build: () -> V) -> V:
        cached = self.get(key)
        if cached is not none and self.is_fresh(key):
            return cached
        value = build()
        self.put(key, value)
        return value

trait Buildable:
    classmethod from_default(cls) -> Self
```
`Cache`'s small required core — `get`, `put`, `is_fresh` — is
exactly what `get_or_put` needs to build the rest of a usable cache
on top of it. Both live in one place: the obligations a type must meet
and the behavior it gets for meeting them are one declaration, read
top to bottom, not two cross-referencing ones.

A classmethod obligation reads the same way as a method one, body or
no body deciding which it is:

```python
trait Countable:
    classmethod zero(cls) -> Self
    classmethod one(cls) -> Self

    classmethod count(cls, n: int) -> Self:
        total = cls.zero()
        for _ in range(n):
            total = total + cls.one()
        return total
```
In Python, the same thing takes two separate tools — an `ABC` for the
required core, a mixin class for the derived behavior — and the `ABC`
half imposes `ABCMeta` on every subclass, which can collide with a
metaclass a class already needs for something else:

```python
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
```
This fails unless `ModelMeta` is made compatible with `ABCMeta` —
the obligation has forced a metaclass choice onto the class. Lucid's
`Sized` above needs no metaclass and no decorator: a body is what
turns a member into an implementation rather than a requirement, so a
class with any remaining bodyless member from a trait or parent class
is abstract for construction purposes, checked before it can be
instantiated, the same useful guarantee `ABCMeta` gives without
requiring one.

## Trait conflicts

Two traits can name the same member without colliding, as long as at
most one of them supplies a body. An obligation is satisfied by
whichever default provides it — nothing to resolve:

```python
trait Renderable:
    def render(self) -> str

trait DebugRenderable(Renderable):
    def debug(self) -> str:
        return "<debug " + self.render() + ">"

class Widget(Renderable, DebugRenderable):
    def render(self) -> str:
        return "widget"
```
`Widget` provides `render`, which satisfies `Renderable`'s
obligation and is exactly what `DebugRenderable.debug` was already
written to call — one implementation, two traits both accounted for,
nothing to reconcile.

Two *defaults* for the same member are the real conflict: two traits
each supplying a body means two candidate implementations, and nothing
says which one a class using both should get. The class must resolve
it explicitly:

```python
trait A:
    def greet(self) -> str:
        return "hello from A"

trait B:
    def greet(self) -> str:
        return "hello from B"

class C(A, B):
    override def greet(self) -> str:
        return A.greet(self)
```
## Field, getter, and setter obligations compose

A field, a getter, and a setter obligation are each a claim about one
capability — read access, write access, or both — and satisfaction
follows the same width-subtyping already used for mutable, read-only,
and immutable views ([Mutable, read-only, and immutable views](mutability.md)): whatever provides at least the capability asked
for satisfies the obligation, the same way a mutable `T` is usable
wherever the narrower `~T` is expected.

A stored field provides both read and write access, so it satisfies a
`getter`-only obligation, a `setter`-only obligation, or a plain
field obligation, on its own:

```python
trait Readable:
    getter x(self) -> float

class Point:
    x: float   # satisfies Readable: reading a field needs no write access
```
A `getter` and `setter` pair together provide exactly what a plain
field does — read and write, nothing more — so the pair satisfies a
plain field obligation the same way a field satisfies the pair:

```python
trait Located:
    x: float

class ComputedPoint:
    getter x(self) -> float:
        return self._x

    setter x(self, value: float):
        self._x = value
```
A `getter` alone does not satisfy a `setter` obligation, or the
reverse: read and write are independent capabilities, and neither
implies the other.

### Final field obligations

A `final` field obligation does not compose the way plain field,
getter, and setter obligations did above — it asks for something
stronger. `final` only promises that the binding is never rebound to
a different object; it says nothing about that object's own contents
(see [Final fields](class-members.md)). A `getter` does not satisfy it,
even alone with no setter:

```python
trait HasModel:
    final model: InferenceModel

class FixedModel:
    final model: InferenceModel   # satisfies HasModel

class RebuiltModel:
    getter model(self) -> InferenceModel:   # does not satisfy HasModel
        return InferenceModel(self.weights, self.metadata, {:})
```
A getter's signature only promises no direct external write — nothing
about it rules out `RebuiltModel` handing back a different object on
every call, which is exactly what `final` rules out. A `getter`
and `setter` pair satisfies it even less: a setter is an explicit
write path, and `final` rules out any write path existing at all.
There is currently no way to mark a getter as provably returning the
same object every call, so a `final` field obligation can only be
satisfied by a `final` field.

Every class is checked for unimplemented obligations before it can be
constructed. A class with any remaining bodyless member from a trait
or parent class is abstract for construction purposes, matching
Python's useful abstract-class instantiation check without using
decorators. Lucid gets that check without requiring classes to inherit
from `ABC` or take on `ABCMeta` as a metaclass.

## Multiple inheritance in Python

Python multiple inheritance uses the same base-class list for several
different jobs: *class inheritance*, obligations, reusable behavior,
metaclass selection, and MRO construction. Those jobs interfere with
each other.

Common pitfalls include:

* MRO order changes which implementation a method call reaches
* cooperative `super()` only works when every class in the chain follows the
  same calling convention
* base classes can bring incompatible constructor requirements

### Cooperative `super()`

Cooperative multiple inheritance requires every class in the chain to accept
and forward compatible arguments. One class that does not participate breaks
the chain.

```python
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
```
`Audited.save` forwards `audit` to `Timestamped.save`, but
`Timestamped.save` does not accept that keyword. The method chain only works
when every participant follows the same forwarding convention.

### Incompatible constructors

Base classes can require incompatible initialization protocols.

```python
class FileBacked:
    def __init__(self, path: Path):
        self.path = path

class NetworkBacked:
    def __init__(self, host: str, port: int):
        self.host = host
        self.port = port

class Cache(FileBacked, NetworkBacked):
    pass
```
There is no obvious generated constructor for `Cache`. One parent needs a
path, the other needs host and port, and neither constructor explains how to
initialize the other base.

Lucid avoids those pitfalls by separating the roles. A class may use
*class inheritance* — inheriting from at most one other class — because a
class is the only one of the two roles that owns stored data. It can use
any number of traits, since a trait without stored state cannot produce
the collision above, whatever mix of obligations and defaults it
declares. If two traits' defaults collide, the class must resolve the
conflict explicitly.

## Explicit overrides

A method that replaces an inherited implementation — the class's one class
parent's, or a trait's — must be marked `override`:

```python
class Timestamped:
    def save(self):
        self.updated_at = now()

class Document(Timestamped):
    override def save(self):
        super.save()
        write_to_disk(self)
```
The same rule governs `override` as governs variance markers: the checker
warns and offers an autofix while drafting, but the marker must be written
into the source before the API is accepted. That protects against both
directions of the same mistake — a parent or trait gaining a method that
silently starts shadowing an unrelated method of the same name with no
signal anywhere, and an intended override whose name or signature no longer
matches anything, silently becoming an unrelated new method while the
original goes on being called elsewhere.

Because a class has at most one class parent, calling through to the
overridden implementation is unambiguous — `super` always means that one
parent, never a position in an MRO, so it takes no parentheses: there is
nothing to call, only the one parent to name. A linter checks that an `override`
method calls it, but this check is a suggestion, not a rule: some overrides
exist specifically to replace an inherited implementation entirely, such as
a class resolving a conflict between two traits, and the warning can be
suppressed for those.

## Final methods

`final` applies to a method the same way it applies to a field or a
class: the same keyword, one relationship fixed permanently — here, that
the method can be overridden at all.

```python
class Timestamped:
    final def save(self):
        self.updated_at = now()

class Document(Timestamped):
    override def save(self):  # error: save is final
        ...
```
`final` is not valid on a bodyless member. It protects a method's
implementation from being replaced, and an obligation has no
implementation to protect — there is nothing there yet for `final` to
fix in place.

## No structural traits

Lucid traits are nominal. A class satisfies a trait only by explicitly
listing it in the class header — `class Foo(SomeTrait)` — never
merely by happening to define members with matching names and types.
This holds even for the numeric capability traits such as
`SupportsInt`: `int` satisfies them because it explicitly inherits from
them, not because it happens to define `__int__`.

Python's `typing.Protocol` takes the opposite approach: any object with
matching methods satisfies a `Protocol`, whether or not its author ever
intended to make that promise. That costs something real, not just style.

A trait obligation is a promise about behavior, not a description of a
method table, and having a matching method is not the same as making
that promise. Two unrelated methods can share a name and signature by
coincidence without sharing a meaning: `close(self) -> none` on a
file handle releases a resource; `close(self) -> none` on a
negotiation finalizes a deal. Structural matching only ever compares
shape, never intent, so a `Closeable`-shaped structural trait would
silently accept both, and code written for one meaning could be handed
the other with no error at all. Only the author of a class can say
which promise, if any, a method is actually making — which is exactly
what listing the trait by name in the class header does and mere
structural matching cannot.

Nominal declaration is also cheaper to check. Satisfying a trait is
verified once, at the class's own definition, against the fixed, usually
short list of traits that class actually names. Structural satisfaction
has no such fixed point: it would have to be rechecked at every place a
value meets a trait-typed slot, comparing that slot's full member
signatures against whatever value happens to show up there, for as long as
either side might change.

## Implementing a trait after the fact

Nominal satisfaction has a real cost of its own: it normally means writing
`class Foo(SomeTrait)` at `Foo`'s own definition, which is not
possible when `Foo` is a builtin or a third-party type you do not control.
Sometimes an existing type should satisfy a trait its own author never
wrote it against.

`implement` attaches an implementation to a type from outside that type's own
definition:

```python
implement Sized for ThirdPartyBuffer:
    def __len__(self) -> int:
        return self.byte_count
```
`ThirdPartyBuffer` did not declare `Sized` when it was defined; this
`implement` block is what makes it satisfy that trait, checked the same way
any other implementation is — does this block actually provide everything
`Sized` requires.

`implement` is not unrestricted. It is valid only if its author owns the
trait or the type — never neither. That rule, not a runtime check, is
what keeps two unrelated packages from each attaching a different
`implement Sized for ThirdPartyBuffer` and leaving which one wins to import
order: for any trait/type pair, at most one project in the whole
dependency graph is ever eligible to write it, because ownership of each
name is unique. The members an `implement` block adds are also only visible
where the trait itself is imported, not ambiently on every
`ThirdPartyBuffer` everywhere — the same reasoning that removed `global`
and `nonlocal`: nothing about a value's usable surface should change
because of something declared elsewhere that the reader never imported.

Subclassing cannot substitute for this. `class Buf(ThirdPartyBuffer,
Sized): ...` only covers instances constructed through `Buf` — it does
nothing for a buffer that already exists, built by code that is never going
to construct your subclass, which is the case this problem actually shows
up in. It does not compose, either: two independent subclasses of
`ThirdPartyBuffer`, each adding a different capability, cannot later be
combined into one class, because that class would need two class parents,
which class inheritance does not allow ([One class parent](class-inheritance.md)). `implement`
has neither limit — it changes what the original type itself satisfies, for
every instance, and any number of unrelated `implement` blocks for the
same type coexist without ever needing to be reconciled into one class.

Python's `typing.Protocol` solves the same retrofitting problem
differently: any type with matching methods satisfies a `Protocol`
automatically, without its author writing anything at all. That is exactly
the accidental-collision risk described above — a type can match a
`Protocol`'s shape by coincidence, without its author ever intending to
make that promise, and Python has no way to tell an intentional
implementation from a lucky guess. `implement` gets structural typing's actual
benefit — an existing type outside your control can come to satisfy a
trait — without that risk. It is still an explicit, checked declaration,
not a shape match: someone has to write it, the checker verifies it actually
provides what the trait requires, and the ownership rule guarantees
that whoever wrote it was entitled to make that claim about that type.
Retrofitting is possible; accidental retrofitting is not.

## Higher-kinded traits

A trait can be generic over a type constructor instead of an ordinary
type, using `F[_]` (see
[Higher-kinded parameters](generics.md)). Inside such a trait, `Self`
takes its kind from how the trait's own members use it: subscripting
`Self` with a type parameter the member declares itself, rather than one
of the class's own, is what makes `Self` behave as a constructor rather
than an already-saturated type. No separate marker is needed on `Self`
the way one is needed on a free-standing parameter like `F` — the
trait's member list is already the explicit, checked-once contract that
a free-standing generic parameter does not have.

```python
trait Functor:
    classmethod map[A, B](cls, tree: Self[A], f: (A) -> B) -> Self[B]
```
`list` was not defined with `Functor`, so it satisfies it through
`implement`:

```python
implement Functor for list:
    classmethod map[A, B](cls, tree: Self[A], f: (A) -> B) -> Self[B]:
        return [f(x) for x in tree]
```
Bounding a free-standing generic parameter by a higher-kinded trait
gets one signature checked once for every implementer, present and future,
instead of the open set of independently-checked cases the dispatch-based
`tree_map` in [Multiple dispatch](dispatch.md) uses:

```python
def tree_map[F[_]: Functor, A, B](tree: F[A], f: (A) -> B) -> F[B]:
    return F.map(tree, f)
```
Which to reach for is the same question either way: dispatch is enough when
each container's traversal only needs to be locally correct on its own; a
bounded, higher-kinded parameter earns its cost when every implementer,
including ones that do not exist yet, needs to be provably checked against
the same signature.

