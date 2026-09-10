# Generics

Generic parameters carry more than an ordinary type in Lucid: variance is
part of the declaration, a parameter can stand for a type constructor
instead of a type, and a type can be quantified over an unnamed
implementer instead of a named one.

## Definition-site variance

Lucid writes variance on the generic parameter where it is declared:
`out K` for covariant, `in K` for contravariant — the same keywords
Kotlin and C# already use for the same jobs — and both together, `in
out K`, for invariant. If a parameter is written with no marker at
all, the checker warns and the linter fills in whichever of the three
is correct — keeping most of the convenience of inferred variance
during drafting, while still requiring the marker to be written into
the source, and preserved or intentionally changed on future edits,
before the API is accepted.

```python
trait Producer[out K]:
    def get(self: ~Self) -> K

trait Consumer[in K]:
    def put(self, value: K) -> none

class Cell[in out K]:
    value: K
```
`in out K` means invariant: no subtyping relationship between
`Cell[A]` and `Cell[B]` at all, unless `A` and `B` already have one of
their own — not bivariance, which would claim the opposite, that
`Cell[A] <: Cell[B]` and `Cell[B] <: Cell[A]` both hold regardless of
how `A` and `B` relate. That second claim is unsound — it would let a
`Cell[Cat]` and a `Cell[Dog]` each stand in for the other — and
Lucid's grammar has no way to write it: `in` and `out` together only
ever mean "no promise either way," never "a promise both ways."

Python's generic variance is often hidden in library declarations or stubs,
inferred from the current member set rather than written down. Lucid puts
variance on the definition instead because variance is part of the public
contract: if a checker infers it from the current members, an ordinary edit
to a trait can silently change assignability for downstream code —
adding a method that consumes `K` can turn an inferred covariant trait
into an invariant one, breaking users who never touched their code. Writing
the marker up front makes the author choose the intended contract, rather
than letting it shift underneath callers as the trait evolves.

## Getting variance wrong

Marking a parameter with the wrong keyword does not fail to compile —
it type-checks right up until something depends on the mistake. Suppose
`Consumer` above were declared covariant instead of contravariant:

```python
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
`Consumer[Dog]` a subtype of `Consumer[Animal]`, so `feed` can pass
it a `Cat`. Covariance is sound only for a parameter that appears in
*output* positions; `put`'s `value: K` is a pure input, so the
soundness direction runs the other way. `in K` gives the checker the
subtyping relation that actually matches the capability on offer:
`Consumer[Animal] <: Consumer[Dog]`, since something that can consume
any `Animal` can stand in wherever something that consumes only `Dog`
is needed — the same shape as a function parameter itself.

A parameter used both ways — read back out somewhere, fed in somewhere
else — cannot be sound at either keyword alone and needs both,
`in out K`, the same reasoning `Cell[in out K]` above already applies
to a field that is both read and written.

## Variance under `~T` and `!T`

A variance keyword is written once, on the mutable type, but a type
with [mutable, read-only, and immutable views](mutability.md) has
three variances to account for, not one. `~T`'s members are always a
subset of `T`'s — every mutating method drops out, nothing is ever
added — so removing members can only remove a use of the parameter,
never introduce one. That gives a directional result: if `T[K]` is
`out K` or `in K`, `~T[K]` and `!T[K]` are forced to match it exactly,
since a subset of "no member consumes `K`" is still "no member
consumes `K`," and the same holds for "no member produces it." There
is nothing left to compute or declare in either case.

Only `in out K`, invariant, leaves the views open. Invariance means
some member produces `K` and some member consumes it — possibly the
same member, possibly two different ones — and whether those survive
onto `~T` depends on whether they happen to be mutating:

```python
class Bag[in out K]:
    def contains(self: ~Self, item: K) -> bool:   # non-mutating, consumes K
        ...

    def pop(self) -> K:                             # mutating, produces K
        ...
```
`Bag` is invariant: `pop` produces `K`, `contains` consumes it.
`~Bag` drops `pop` — mutating — and keeps `contains` — it doesn't
mutate anything — so the only surviving use of `K` is as an input.
That leftover `in` use is sound: something that can check membership
against any `Animal` can stand in wherever checking membership
against only `Dog` is needed. [Read-only dictionaries](mutability.md)
shows the opposite outcome for the same reason in reverse — a
mutating consumer drops out, leaving only a producer behind, and the
view loosens to covariant instead.

Since an invariant mutable type can loosen to covariant, to
contravariant, or stay invariant under its views, and which of the
three isn't knowable from `in out` alone, a `~` on one of the two
keywords names the outcome directly — it marks the half that does
*not* survive to the read-only and immutable views. `Bag` above is
really `class Bag[in ~out K]`: `out` drops, `in` survives. [Safe
covariance](mutability.md#safe-covariance)'s `InferenceModel` is the
opposite case, `~in out K`: `in` drops, `out` survives. Together with
plain `out K` and `in K`, this covers every reachable combination —
the other four of the nine naively possible (mutable, view) pairings,
such as a covariant mutable type with an invariant view, can never
happen, so there is no marker for them:

| Marker | Mutable | `~T` / `!T` |
| --- | --- | --- |
| `out K` | covariant | covariant (forced) |
| `in K` | contravariant | contravariant (forced) |
| `in out K` | invariant | invariant |
| `~in out K` | invariant | covariant |
| `in ~out K` | invariant | contravariant |

`~T` and `!T` always land on the same variance as each other:
freezing constrains what a value's fields *store*, not which methods
exist or how they use `K`, so `!T`'s callable member set is exactly
`~T`'s. One keyword pair on the mutable declaration settles all
three views.

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

## Use-site projection

`~T` and `!T` narrow *every* parameter's role at once: they drop any
method that needs mutable `self`, for any reason, regardless of what
it touches. Sometimes only one parameter's role needs narrowing,
leaving the rest exactly as declared. `list` is invariant in its
element type — it has both `__getitem__` and `__setitem__` — but a
caller who only ever wants to *fill* a `list` in, never read it back,
can say so directly at the annotation, without asking for `~list` or
`!list` and without `list` needing a second, purpose-built type:

```python
def recv_into(sock: Socket, buf: list[in int]) -> int:
    ...   # can write ints into buf; cannot read buf's existing contents
```
`list[in int]` drops every method that *reads* `int` back out —
`__getitem__` — while `__setitem__`, `append`, and `__len__` stay
exactly as `list` already declares them, mutable and all. `out` runs
the same rule the other way, dropping methods that *consume* the
named parameter instead of producing it — the projection this
recovers is the read-only one `~list[int]` already gives, for a
single-parameter container whose only mutable state is the parameter
itself.

For a class with more than one parameter, the projection restricts
just the one named, positionally, the same way the parameter itself
is written — leaving every other parameter at whatever `Node` itself
declares for it:

```python
class Node[Data, Children]:
    data: Data
    children: list[Children]

    def set_data(self, d: Data) -> none:
        self.data = d

    def add_child(self, c: Children) -> none:
        self.children.append(c)

def update_payloads(nodes: list[Node[int, out str]]) -> none:
    for node in nodes:
        node.set_data(5)      # fine: Data is unrestricted
        node.add_child("x")   # error: add_child consumes Children, and Children is out here
```
`Node[int, out str]` leaves `Data` exactly as declared — fully
mutable — while dropping every method that writes `Children`. `~T`
and `!T` cannot express this at all: they are whole-object operators,
with no way to restrict one parameter while leaving another
untouched. Use-site projection is the narrower, complementary tool
for exactly that case, not a replacement for `~T`/`!T`, which stay
the right choice whenever the restriction really is "no mutation, for
any reason."

For a single-parameter type whose only mutable state is the
parameter itself — true of `list`, `dict`, `set`, and any ordinary
container — `T[out X]` and `~T[X]` land on the same callable set,
since "mutating" and "produces/consumes `X`" happen to be the same
fact there, and `~T[X]` is the one to reach for. The projection earns
its keep once a class has mutable state unrelated to the parameter
being restricted, or more than one parameter to restrict
independently, as `Node` does above.

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
never their union. `in`, written *after* the parameter's name — the
same position-based split [variance's own `in`/`out`](#definition-site-variance)
already relies on, just on the other side of the name — declares that:

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
layer a subtype constraint onto. It composes with variance the same
way basedpython's own version does, since the two markers sit on
opposite sides of the name: `class Container[in out T in (int, str)]:`
declares `T` both invariant and restricted to exactly `int` or `str`.

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
this is the same safe narrowing [Getting variance wrong](#getting-variance-wrong)
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
The same rule that governs variance markers governs `F[_]`: it must be
written explicitly on a free-standing generic parameter like `F` above,
even though the checker could infer the arity from `tree: F[A]` during
drafting, because an unrelated later edit to `tree_map`'s body could
otherwise silently change what `F` is required to be — exactly the danger
explicit variance markers already exist to rule out.

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


