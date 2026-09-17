# Generics

Generic parameters carry more than an ordinary type in Lucid: variance is
computed from how each parameter is actually used, a parameter can stand
for a type constructor instead of a type, and a type can be quantified
over an unnamed implementer instead of a named one.

## Inferred variance

A generic parameter's variance is never written down: the checker derives
it directly from how the parameter is used across the class or trait's
own body. A parameter that appears only in *output* positions — a return
type, a readable field — is covariant. One that appears only in *input*
positions — a method parameter, a writable field — is contravariant. One
used both ways is invariant, and one that appears nowhere in the public
interface is unconstrained either way, so the checker treats it as
covariant, the same default an omitted type argument already gets (see
[Leaving a type parameter unspecified](#leaving-a-type-parameter-unspecified)).

```python
trait Producer[K]:
    def get(self: ~Self) -> K   # output only: covariant

trait Consumer[K]:
    def put(self, value: K) -> none   # input only: contravariant

class Cell[K]:
    value: K   # read and written: invariant
```
`Cell[K]` is invariant: there is no subtyping relationship between
`Cell[A]` and `Cell[B]` at all, unless `A` and `B` already have one of
their own — not bivariance, which would claim the opposite, that
`Cell[A] <: Cell[B]` and `Cell[B] <: Cell[A]` both hold regardless of how
`A` and `B` relate. That second claim is unsound: it would let a
`Cell[Cat]` and a `Cell[Dog]` each stand in for the other. A parameter
used both ways derives to invariance instead, never bivariance — "read
back out somewhere, fed in somewhere else" is a promise that holds in
neither direction alone, not a promise that holds in both at once.

Kotlin, C#, and basedpython instead ask the author to write `out K`/
`in K` on the declaration, treating variance as a contract the author
states rather than a fact the compiler can already see from the body.
That guards against a checker whose inference runs once, at whatever
moment a library's stub is written, and is never rechecked against how
the class is actually used afterward — the real risk behind Python's own
stub-inferred variance: an ordinary edit that adds a consuming method can
turn a covariant class invariant, and nothing re-verifies the stub
against its real callers to catch it. Lucid's checker has no such gap:
it re-derives a parameter's variance from the same class body on every
check, in the same pass that re-verifies every consumer against the
result, so a member that flips a parameter's variance surfaces as a
compile error at whichever call site depended on the old one — caught
where the class changed, not learned later at a caller three files away.

## Only one direction is sound

A parameter used only as an input can be assigned in exactly one safe
direction, and Lucid's checker derives that direction instead of trusting
an author to choose it. In Kotlin, C#, or basedpython, marking such a
parameter with the wrong keyword does not fail to compile — it
type-checks right up until something depends on the mistake:

```python
# hypothetical: Consumer marked covariant, as a Kotlin/C#/basedpython author might do by mistake
trait Consumer[out K]:      # wrong: put(value: K) only consumes K
    def put(self, value: K) -> none

class Dog(Animal): ...
class Cat(Animal): ...

def feed(c: Consumer[Animal]) -> none:
    c.put(Cat())

dog_feeder: Consumer[Dog] = ...
feed(dog_feeder)  # accepted under out K: Consumer[Dog] <: Consumer[Animal]
```
`dog_feeder` only knows how to `put` a `Dog`, but `out K` makes
`Consumer[Dog]` a subtype of `Consumer[Animal]`, so `feed` can pass it a
`Cat`. Covariance is sound only for a parameter that appears in *output*
positions; `put`'s `value: K` is a pure input, so the soundness direction
runs the other way.

Lucid has no keyword here to get wrong. [Inferred variance](#inferred-variance)
sees that `Consumer[K]`'s only member consumes `K` and derives
contravariance directly: `Consumer[Animal] <: Consumer[Dog]`, since
something that can consume any `Animal` can stand in wherever something
that consumes only `Dog` is needed — the same shape as a function
parameter itself, and the only direction `feed`'s call above could ever
type-check under. A parameter used both ways — read back out somewhere,
fed in somewhere else — can't be sound at either extreme, which is
exactly why [Inferred variance](#inferred-variance) derives `Cell[K]` as
invariant instead.

## Variance under `~T` and `!T`

A type with [mutable, read-only, and immutable views](mutability.md) has
three variances to account for, not one, and [Inferred variance](#inferred-variance)'s
analysis runs again for each: once against `T`'s full member set, and
again against whichever subset of it survives on `~T` and `!T`. `~T`'s
members are always a subset of `T`'s — every mutating method drops out,
nothing is ever added — so removing members can only remove a use of the
parameter, never introduce one. That gives a directional result: if `T`
is covariant or contravariant, `~T` and `!T` are forced to match it
exactly, since a subset of "no member consumes `K`" is still "no member
consumes `K`," and the same holds for "no member produces it."
[`Producer`](#inferred-variance) stays covariant on every view;
[`Consumer`](#inferred-variance) stays contravariant on every view.

Only an invariant `T` can genuinely diverge, since invariance means some
member produces `K` and some member consumes it — possibly the same
member, possibly two different ones — and whether those survive onto
`~T` depends on whether they happen to be mutating:

```python
class Bag[K]:
    def contains(self: ~Self, item: K) -> bool:   # non-mutating, consumes K
        ...

    def pop(self) -> K:                             # mutating, produces K
        ...
```
`Bag` is invariant: `pop` produces `K`, `contains` consumes it. `~Bag`
drops `pop` — mutating — and keeps `contains` — it doesn't mutate
anything — so the only surviving use of `K` is as an input, and
[Inferred variance](#inferred-variance)'s own analysis, run again against
that reduced set, derives `~Bag[K]` as contravariant. That leftover use
is sound: something that can check membership against any `Animal` can
stand in wherever checking membership against only `Dog` is needed.

[Safe covariance](mutability.md#safe-covariance)'s `InferenceModel` shows
the opposite outcome: `score` writes to `self._scores`, consuming `K`, so
it drops out of `~InferenceModel`, leaving only `labels: list[K]`'s read
behind — the checker derives `~InferenceModel[K]` as covariant even
though `InferenceModel[K]` itself is invariant.

[Read-only dictionaries](mutability.md#read-only-dictionaries)'s
`dict[K, V]` shows two parameters diverging independently in the same
declaration. `K` stays invariant everywhere: both of its uses — `get`'s
lookup and `keys()`'s enumeration — are non-mutating, so both survive
onto the read-only view unchanged, and invariance survives with them. `V`
is only ever produced by a non-mutating member (`get`) and only ever
consumed by a mutating one (`__setitem__`), so the read-only view drops
the consuming use and `V` derives as covariant alone. A narrower view
that exposes only keys or only values can land on different variance
again, for the same reason.

`~T` and `!T` land on the same variance as each other whenever every
method reachable only through `!Self` leaves `K` alone — true of
every example above, and of the built-in [`Hashable`](mutability.md#equality-ordering-and-hashing):
`__hash__(self: !Self) -> int` needs the deep-freeze guarantee for an
unrelated reason, a stable hash, and never mentions `K` in its own
signature. A `self: !Self`-only method that did use `K` would be
callable on `!T` but not on `~T`, so the two could in principle
disagree; nothing here rules that out, but no example needs it, so
it stays unaddressed until one does. The class body settles all three
views for every case in this document — nothing else needs writing.

A private field or method never enters this computation at all,
regardless of how it uses `K`. Variance is a promise about the
*public* interface — whether `Foo[Dog]` can stand in for `Foo[Animal]`
from the outside — and no outside caller can ever reach a private
member to exploit an unsound substitution through it, since
[private members are enforced](class-inheritance.md#private-members),
not merely named by convention. [The worked example](index.md#example)'s `InferenceModel`
relies on exactly this: `score` mutates a private `_scores` cache, but
that write only has to be reconciled with whatever `score` and
`labels` themselves expose publicly, not treated as a use in its own
right.

## Leaving a type parameter unspecified

Python's bare `list`, with no type argument, is usually treated as
`list[Any]` — and `Any` disables checking for both reads and
writes, not just the one where information is actually missing. Lucid
has no `Any` for a bare name to fall back to, so leaving a type
parameter unspecified can mean something sound instead, computed by
the same per-parameter variance already used above: a covariant
parameter reads as `object`; a contravariant one is `Never`,
accepting nothing; an invariant one — `list`'s own case, since it is
both read and written — reads as `object` and cannot be written at
all:

```python
def describe(items: list) -> str:
    return str(items[0])   # fine: element type unspecified, reads as object
    items[0] = 1            # error: list is invariant, and unspecified here
```
This gives an unspecified `list` the same three-way split `~T`/
`!T` already have above, just triggered by omission instead of a
view marker: whatever a parameter's variance would force a view to
become, leaving it out entirely forces the same way. Some languages
need a separate, deliberately-chosen spelling for exactly this — a
sound alternative to a bare name's own unsound default. Lucid has no
unsound default to be distinct from, so there is nothing separate to
opt into: leaving a parameter unspecified already means it.

## Bounds across type parameters

A bound can name another type parameter already in scope — one that
precedes it in the same list, or one belonging to an enclosing list:

```python
def pick[T, R: T](t: T, r: R) -> T:
    return t

class Owner[T]:
    def narrow[U: T](self, u: U) -> T:
        return u
```
The bound takes part in solving a call rather than being checked
against one argument at a time, so `R: T` is a floor under `T`, not a
constraint on `R` in isolation:

```python
class Animal: ...
class Dog(Animal): ...

pick(Dog(), Animal())   # T is Animal, not Dog
```
`t: Dog` alone would infer `T` as `Dog`, but `r: Animal` has to satisfy
`R: T` too, and `Dog` isn't a floor under `Animal` — so the solver
widens `T` to `Animal`, the narrowest type both arguments actually fit.
Nothing else has to mention `T` for the checker to find it, either —
`R`'s own bound is enough on its own:

```python
def only_bound[T, R: T](r: R) -> T:
    return r

only_bound(1)   # T is Literal[1]
```
A name that isn't yet in scope is rejected: a later parameter in the
same list, and a parameter's own name inside its own bound, are both
out of reach, the same way an ordinary variable can't appear in its
own initializer:

```python
def f[S: T, T](s: S, t: T): ...    # error: T comes later
def g[T: list[T]](x: T) -> T: ...  # error: T is not in scope inside its own bound
```

## Type parameter defaults

A type parameter can default to a type instead of requiring one at
every use site: `class Foo[T = int]:` means a bare `Foo`, with no
subscript at all, is `Foo[int]`. `Self` is a valid default too, and
it is the one that earns its keep — a class generic over "whatever
type one particular method actually produces," defaulting to the
receiver's own type for the common case, without forcing every
subclass to name itself explicitly just to get that:

```python
trait AbstractContextManager[T = Self]:
    contextmanager def __cm__(self) -> T
```
Most context managers do yield themselves, so the default covers the
common case for free:

```python
class FileHandle(AbstractContextManager):
    def close(self) -> none:
        ...

    contextmanager def __cm__(self) -> Self:
        yield self
        self.close()
```
But not every context manager yields itself — a stream redirector
yields the stream it's redirecting *to*, not the redirector; a
connection's own `transaction()` often yields a cursor, not the
connection. `T`'s default isn't a bound, so nothing requires `__cm__`
to agree with `Self` at all; an explicit type argument simply
replaces the default instead of having to satisfy it:

```python
class Redirect(AbstractContextManager[TextIO]):
    target: TextIO

    contextmanager def __cm__(self) -> TextIO:
        old = sys.stdout
        sys.stdout = self.target
        yield self.target
        sys.stdout = old
```
`Self` can be a default but not a bound. A bound is checked at
specialization — does `X` satisfy `T: SomeBound` when a caller writes
`Foo[X]`? — and that check has no receiver to resolve `Self` against;
specializing a class is not itself a call on an instance. A default
has no such moment to answer for: it stays symbolic until a member is
actually looked up on a receiver, the same way `Self` already works
everywhere else, so there is always a concrete class to resolve it
against by the time it matters:

```python
trait Bad[T: Self]: ...   # error: Self cannot bound a type parameter of the class it belongs to
```

## Ranging over a fixed set of types

A bound, `T: X`, lets `T` be `X` or any subtype of it. Sometimes the
intent is narrower still: `T` should be exactly one member of a fixed,
small set, chosen fresh each call, never a subtype of one member and
never their union. `in`, written *after* the parameter's name, declares
that:

```python
def concat[T in (str, bytes)](a: T, b: T) -> T:
    return a + b

concat("x", "y")     # T is str
concat(b"x", b"y")   # T is bytes
concat("x", b"y")    # error: "x" and b"y" share no single member of the set
```
An ordinary bound wouldn't catch the mismatched call at all: `T: str |
bytes` lets `T` widen to the union when arguments disagree, and
whether that ever surfaces as an error depends entirely on whether the
function body happens to do something that demands a concrete type.
One that doesn't type-checks fine with mismatched arguments regardless:

```python
def first[T: str | bytes](a: T, b: T) -> T:
    return a

first("x", b"y")   # type-checks: T widens to str | bytes, and first
                    # never needs a concrete type to return a unchanged
```
`in (str, bytes)` rejects the mismatch directly, at the call site,
regardless of what the function does with its arguments — a real
difference, not just a clearer spelling of the same check.

A set needs at least two members — one leaves nothing to choose
between — and it is an alternative to an ordinary bound, not an
addition to one: a parameter takes either `T: X` or `T in (X, Y, ...)`,
never both, since a set of alternatives has no single upper bound to
layer a subtype constraint onto. It composes with inferred variance
without conflict, since the two are independent facts about a
parameter: `class Container[T in (int, str)]:` restricts `T` to
exactly `int` or `str`, while the checker still derives whatever
variance `Container`'s own members give it.

## Member types

A type parameter's own members can be named in a type position: `T.a`
is the type of member `a` on whatever `T` turns out to be, not
flattened to whatever `a` happens to be on `T`'s own bound. `T` isn't
known yet where the annotation is written, so the lookup stays
symbolic — much like [literal arithmetic](type-operations.md#arithmetic-on-literal-types)
on a type parameter — and re-resolves at each specialization, so a
subclass that narrows a member is honored rather than flattened to
the bound's own declaration:

```python
class Animal:
    getter offspring(self: ~Self) -> Animal:
        ...

class Dog(Animal):
    getter offspring(self: ~Self) -> Dog:
        ...

class Nursery[T: Animal]:
    resident: T
    latest: T.offspring

def check(n1: Nursery[Animal], n2: Nursery[Dog]) -> none:
    n1.latest   # Animal
    n2.latest   # Dog
```
`offspring` narrows covariantly in `Dog` — a `getter` is read-only, so
this is the same safe narrowing [Only one direction is sound](#only-one-direction-is-sound)
already establishes for any output-only position; a plain, mutable
field couldn't narrow this way; a caller holding `resident` through
its `Animal`-typed slot could otherwise write an `Animal` into what is
actually a `Dog`'s storage.

The receiver doesn't have to be a bare parameter. Any type expression
works, including one already specialized, so a member's type can be
asked for directly, and receivers chain:

```python
Nursery[Dog].latest         # Dog
Nursery[Dog].latest.offspring  # Dog, chained through Dog's own offspring
```
A member the bound doesn't have is an error, checked where `T.a` is
written, not only once `T` is specialized:

```python
class B[T: Animal]:
    x: T.nope   # error: Animal has no attribute `nope`
```
[Ranging over a fixed set of types](#ranging-over-a-fixed-set-of-types)
changes what a member type means before specialization: for an
ordinary bound, `T.a` reads as the bound's own `a` until `T` narrows
further. For `T in (X, Y)`, there is no single bound to fall back on
— each specialization picks exactly one member, so `T.a` in a value
position unions over what every member's `a` could be:

```python
class Kennel[T in (Animal, Dog)]:
    resident: T
    latest: T.offspring   # Animal | Dog, until T is specialized
```

## Higher-kinded parameters

An ordinary generic parameter like `K` above stands for a type. Some
generic code needs a parameter that stands for a type *constructor*
instead — something that is not itself a complete type until it is applied
to one, the same way a plain function is not a value until it is called.
`F[_]` declares that: a generic parameter that takes exactly one type
argument to become concrete, written with the same subscript syntax
ordinary type application already uses (`F[_, _]` for a constructor that
takes two, and so on). The `_` is the same black-hole marker used
elsewhere for a binding that does not need a name (see
[Binding](names.md)) — here, in type
position, it means a type-argument slot the declaration does not need to
name either.

```python
def tree_map[F[_]: Functor, A, B](tree: F[A], f: (A) -> B) -> F[B]:
    return F.map(tree, f)
```
`F[_]` must be written explicitly on a free-standing generic parameter
like `F` above, even though the checker could infer the arity from
`tree: F[A]` during drafting, because an unrelated later edit to
`tree_map`'s body could otherwise silently change what `F` is required
to be — the same danger [Explicit overrides](traits.md#explicit-overrides)
already exists to rule out for `override`.

## Existential types

An ordinary trait can already be used directly as a type — `count:
SupportsIndex` already means "any type satisfying `SupportsIndex`, caller's
choice, checker doesn't care which." That is existential quantification,
even though it is ordinary enough that nothing here has called it that
until now.

It stops working for a higher-kinded trait like `Functor`, because
`Functor` is not itself a complete type — the same reason bare `list`
is not. What a recursive alias like `PyTree` wants to say is "a value of
type `F[PyTree[L]]`, for *some* `F` satisfying `Functor`," which
bundles two things an ordinary trait-as-type never had to: the
constraint, and what it is applied to. `any` spells that:

```python
type PyTree[L] = L | any Functor[PyTree[L]]
```
`any Functor` stands in for an unnamed `F` satisfying `Functor`, the
same role `Self` plays inside `Functor`'s own declaration, just
existentially bound instead of referring back to whatever class the
declaration is already inside. Subscripting it, `any Functor[PyTree[L]]`,
applies that stand-in the same way `F[PyTree[L]]` would if `F` were a
real, named, universally-quantified parameter — the two are duals of each
other, one saying "works for every `F`," the other "holds some particular
`F`, unspecified."

This costs nothing at runtime. Every value already carries its own
concrete class, a fact [Multiple dispatch](dispatch.md) relies on
too, later in this reading order — so a value of type `any
Functor[X]` needs no extra representation;
whatever concretely [implement](traits.md)\ s `Functor` is
already dispatchable the ordinary way. The only new thing `any` asks of
the checker is to accept any concrete `F[X]` under one annotation for a
higher-kinded trait, the same courtesy it already extends to ordinary
ones.


