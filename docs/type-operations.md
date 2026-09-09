# Operations

## Union types

`A | B` is the type of values that are an `A` or a `B`, the same
spelling Python's own `X | Y` union syntax already settled on —
already used throughout [Type vocabulary](types.md), `int | none`
among its first examples.

## Intersection types

`A & B` is the type of values satisfying both `A` and `B` — the
dual of the union `A | B` just covered.
Lucid's traits are nominal ([Main ideas](principles.md)): a
promise is made only where a class header names it, so a value
assembled from two independently declared traits has nowhere to go
without either a purpose-declared class combining both up front, or a
type that can say "both, whichever value actually provides them":

```python
def render(shape: Drawable & Serializable) -> bytes:
    ...

handlers: list[HasName & HasId]
```
`&` means only this — the read-only mutability view prefix used
throughout [Mutability](mutability.md) (`~Self`, `~dict[K, +V]`) was
moved to `~` for exactly this reason, so `&` never has to carry two
meanings at once.

Neither `&` nor `|` has a keyword alternate. `and` and `or`
stay exactly what they already are — value-level boolean operators —
and are never meaningful in a type position; there is one spelling for
each combinator, not two.

## Negation types

`not T` in a type position is the type that excludes `T` — every
value that is not one:

```python
def f(x: not int) -> none:
    ...

f("a")   # ok
f(1)     # error
```
`not` already exists, as ordinary boolean negation; a type position
reads it as type negation instead, the same way
[Type vocabulary](types.md)'s own type-expression grammar already
gives several other tokens a second meaning by position rather than by
a second word — `type`, disambiguated the same way
([What type means in Lucid](types.md#what-type-means-in-lucid)).
`not int | str` and `not (int | str)` differ exactly the way they
read: the first excludes only `int`, leaving `str` untouched by the
negation, the second excludes the whole union. Nesting follows the
same rule as every other type combinator here: parenthesize to bind
negation across `|` or `&`; unparenthesized, it binds to the type
immediately next to it.

## Arithmetic on literal types

`+`, `-`, and `*` between two `Literal[int]` types compute a new
`Literal[int]` type, the same rule that already lets `&`, `|`, and
`not` mean something different in a type position than they do as
ordinary value-level operators:

```python
type Product[A: int, B: int] = A * B
```
`Product[3, 4]` is `Literal[12]` — computed once, at the type level,
the same way `promote[A, B]` (below) computes a type instead of
substituting one. Comparing two computed literals is ordinary type
equality, the same check that already decides whether any other two
types match.

## Promotion

`promote[A, B]` is a parameterized type like any other — `Array[A]`,
`list[A]`, `PyTree[L]` — living in the same type-expression grammar
every generic type already does. Nothing about *where* it lives is
new; what is new is *how* it resolves: given two
[`Promotes`](dispatch.md#promotion) types, to whichever one is already
listed in the other's `promotes_to`. Since each type's `promotes_to`
is already its full promotion set rather than one step of it, that
membership test is the entire computation — no walk, no recursion,
just the ordinary `in` operator reused in type position, the same way
`+`/`-`/`*` were reused for literal arithmetic above:

```python
type promote[A: Promotes, B: Promotes] =
    B if B in A.promotes_to else
    A if A in B.promotes_to else
    error   # A and B share no common promotion target
```
`int32 + complex64` resolves to `complex64` directly, because
`int32.promotes_to` already contains `complex64` — not because
anything walked there through `float32` first. The two `if`/`else`
branches are symmetric on purpose: `A in B.promotes_to` covers the
case where `B` is actually the narrower type, so `promote[int32,
complex64]` and `promote[complex64, int32]` resolve the same way
regardless of which argument came first.

