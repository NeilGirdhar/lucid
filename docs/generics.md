# Generics

Generic parameters carry more than an ordinary type in Lucid: variance is
part of the declaration, a parameter can stand for a type constructor
instead of a type, and a type can be quantified over an unnamed
implementer instead of a named one.

## Definition-site variance

Lucid writes variance on the generic parameter where it is declared: `+K`
for covariant, `-K` for contravariant, and `=K` for invariant. If a
parameter is written without a variance marker, the checker warns, infers
the narrowest valid variance, and offers an autofix — keeping most of the
convenience of inferred variance during drafting, while still requiring the
marker to be written into the source, and preserved or intentionally
changed on future edits, before the API is accepted.

```python
trait Producer[+K]:
    def get(self) -> K

trait Consumer[-K]:
    def put(self, value: K) -> none

class Cell[=K]:
    value: K
```
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

Marking a parameter with the wrong variance does not fail to compile —
it type-checks right up until something depends on the mistake. Suppose
`Consumer` above were declared covariant instead of contravariant:

```python
trait Consumer[+K]:      # wrong: put(value: K) only consumes K
    def put(self, value: K) -> none

class Dog(Animal): ...
class Cat(Animal): ...

def feed(c: Consumer[Animal]) -> none:
    c.put(Cat())

dog_feeder: Consumer[Dog] = ...
feed(dog_feeder)  # accepted under +K: Consumer[Dog] <: Consumer[Animal]
```
`dog_feeder` only knows how to `put` a `Dog`, but `+K` makes
`Consumer[Dog]` a subtype of `Consumer[Animal]`, so `feed` can pass
it a `Cat`. Covariance is sound only for a parameter that appears in
*output* positions; `put`'s `value: K` is a pure input, so the
soundness direction runs the other way. `-K` gives the checker the
subtyping relation that actually matches the capability on offer:
`Consumer[Animal] <: Consumer[Dog]`, since something that can consume
any `Animal` can stand in wherever something that consumes only `Dog`
is needed — the same shape as a function parameter itself.

A parameter used both ways — read back out somewhere, fed in somewhere
else — cannot be sound at either variance and needs `=K`, invariant,
the same reasoning `Cell[=K]` above already applies to a field that is
both read and written.

## Variance under `~T` and `!T`

A variance marker is written once, on the mutable type, but a type with
[mutable, read-only, and immutable views](mutability.md) has three
variances to account for, not one. `~T`'s members are always a subset
of `T`'s — every mutating method drops out, nothing is ever added — so
removing members can only remove a use of the parameter, never introduce
one. That gives a directional result: if `T[K]` is `+K` or `-K`,
`~T[K]` and `!T[K]` are forced to match it exactly, since a subset of
"no member consumes `K`" is still "no member consumes `K`," and the
same holds for "no member produces it." There is nothing left to compute
or declare in either case.

Only `=K`, invariant, leaves the views open. Invariance means some
member produces `K` and some member consumes it — possibly the same
member, possibly two different ones — and whether those survive onto
`~T` depends on whether they happen to be mutating:

```python
class Bag[=K]:
    def contains(self: ~Self, item: K) -> bool:   # non-mutating, consumes K
        ...

    def pop(self) -> K:                             # mutating, produces K
        ...
```
`Bag` is invariant: `pop` produces `K`, `contains` consumes it.
`~Bag` drops `pop` — mutating — and keeps `contains` — it doesn't
mutate anything — so the only surviving use of `K` is as an input.
`~Bag[-K]` is sound: something that can check membership against any
`Animal` can stand in wherever checking membership against only
`Dog` is needed. [Read-only dictionaries](mutability.md) shows the
opposite outcome for the same reason in reverse — a mutating consumer
drops out, leaving only a producer behind, and the view loosens to
covariant instead.

Since an invariant mutable type can loosen to `+K`, to `-K`, or stay
`=K`, and which of the three isn't knowable from the mutable type's
own marker alone, two more markers name the outcome directly: `+=K`
for invariant-that-loosens-to-covariant, `-=K` for
invariant-that-loosens-to-contravariant. Together with `+`, `-`, and
plain `=`, this covers every reachable combination — the other four of
the nine naively possible (mutable, view) pairings, such as a covariant
mutable type with an invariant view, can never happen, so there is no
marker for them:

| Marker | Mutable | `~T` / `!T` |
| --- | --- | --- |
| `+K` | covariant | covariant (forced) |
| `-K` | contravariant | contravariant (forced) |
| `=K` | invariant | invariant |
| `+=K` | invariant | covariant |
| `-=K` | invariant | contravariant |

`~T` and `!T` always land on the same variance as each other:
freezing constrains what a value's fields *store*, not which methods
exist or how they use `K`, so `!T`'s callable member set is exactly
`~T`'s. One marker on the mutable declaration settles all three views.

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
[Binding, destructuring, and scope](names.md)) — here, in type
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


