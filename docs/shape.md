# Shapes

A numeric array's shape — how many dimensions it has, and how large
each one is — is exactly the kind of fact [Type vocabulary](types.md)
already wants visible at the definition site, the same way mutability
and variance are. `typing.Shape` and `typing.shape[...]` give it a
type, and this covers the library of [match types](match-types.md)
built on top of it for taking one apart, combining two, and checking
the constraints an operation like reshape or matrix multiplication
actually needs.

## `typing.Shape` and `typing.shape[...]`

`typing.shape[2, 3, 4]` borrows its syntax from `Literal[1, 2, 3]` — a
builtin name, subscripted with however many type arguments the caller
writes — but not its meaning. `Literal[1, 2, 3]` is a union: satisfied
by any *one* of the three. `typing.shape[2, 3, 4]` is a product:
satisfied only by exactly that sequence, in that order — the same
fixed-arity, exact-match structure an anonymous record like
`(x: int, y: int)` already has, just with positions instead of names.
`typing.Shape` is the trait every `typing.shape[...]` instantiation
satisfies.

A position ordinarily holds a `Literal[int]`, but any position can
hold a type variable instead, for a dimension whose size isn't fixed
— an ordinary generic parameter sitting in one slot, the same way it
would sit in any other type expression:

```python
def batch_normalize[Batch: int](
    x: Array[Float32, typing.shape[Batch, 3, 224, 224]],
) -> Array[Float32, typing.shape[Batch, 3, 224, 224]]:
    ...
```
An array that doesn't track its shape at all uses `none` instead of a
`typing.shape[...]`, so one class covers both:

```python
class Array[D: DataType, S: typing.Shape | none]:
    ...

checked: Array[Float32, typing.shape[2, 3, 4]]
unchecked: Array[Float32, none]
```
Everything from here on lives in the `typing.shape` module; the
`typing.shape.` prefix is dropped from the definitions below the way
`iteration.done`'s own definition, inside the `iteration` module,
never writes `iteration.` either — only a caller from outside needs
the qualified name.

## Taking a shape apart

[Match types](match-types.md) already showed `Rank`, peeling one
position at a time. `Get` and `DropAt` peel down to a specific index
instead of peeling everything:

```python
type Get[S: Shape, I: int] = match S:
    case shape[Head, *Rest]: match I:
        case 0: Head
        case N: Get[Rest, N - 1]

type DropAt[S: Shape, I: int] = match S:
    case shape[Head, *Rest]: match I:
        case 0: Rest
        case N: shape[Head, *DropAt[Rest, N - 1]]
```
`InsertAt` runs the same walk, splicing a new dimension in instead of
removing one:

```python
type InsertAt[S: Shape, I: int, D: int] = match I:
    case 0: shape[D, *S]
    case N: match S:
        case shape[Head, *Rest]: shape[Head, *InsertAt[Rest, N - 1, D]]
```

## Recombining shapes

`Concat` glues two shapes end to end — prepending a batch shape onto a
fixed trailing shape is the common case — and `Reverse` is built
directly from it:

```python
type Concat[A: Shape, B: Shape] = match A:
    case shape[]: B
    case shape[Head, *Rest]: shape[Head, *Concat[Rest, B]]

type Reverse[S: Shape] = match S:
    case shape[]: shape[]
    case shape[Head, *Rest]: Concat[Reverse[Rest], shape[Head]]
```
Transpose — swapping the last two axes, the case that actually comes
up, as opposed to swapping two arbitrary indices, which composes from
`Get`/`DropAt`/`InsertAt` but needs care once the first removal shifts
the second index — needs no index arithmetic at all, since the
positions it cares about are already at the front once the shape is
reversed:

```python
type SwapLast2[S: Shape] = match Reverse[S]:
    case shape[Last, SecondLast, *Rest]: Reverse[shape[SecondLast, Last, *Rest]]
```

## Batch dimensions

The single most common shape pattern in real numeric code is "any
number of leading batch dimensions, then a fixed trailing shape" —
`[*Batch, 3, 224, 224]`. That's [the same star that already captures a
run of unpacked values](collections.md#unpacking), just with a fixed
suffix following it instead of nothing:

```python
type BatchDims[S: Shape] = match S:
    case shape[*Batch, 3, 224, 224]: Batch

type TrailingShape[S: Shape] = match S:
    case shape[*_, 3, 224, 224]: shape[3, 224, 224]
```

## Broadcasting

Two dimensions are broadcast-compatible if they're equal, or either
one is exactly `1` — a per-position check with no arithmetic in it,
using the same trick `BatchDims` and `promote`'s own `ReachableFrom`
both already rely on: a name already bound (`B` below) becomes an
exact-match pattern the next time it's written:

```python
type BroadcastDim[A: int, B: int] = match A:
    case B: A
    case 1: B
    case _: match B:
        case 1: A
        case _: error   # A and B are not broadcast-compatible
```
Broadcasting aligns from the *right*, the opposite end from where
peeling naturally recurses, so `BroadcastAligned` runs on reversed
shapes, and `Broadcast` reverses the result back:

```python
type BroadcastAligned[A: Shape, B: Shape] = match A:
    case shape[]: B
    case shape[AHead, *ARest]: match B:
        case shape[]: A
        case shape[BHead, *BRest]:
            shape[BroadcastDim[AHead, BHead], *BroadcastAligned[ARest, BRest]]

type Broadcast[A: Shape, B: Shape] = Reverse[BroadcastAligned[Reverse[A], Reverse[B]]]
```

## Matrix multiplication's shape rule

Matmul needs the inner dimensions to match exactly and the batch
dimensions to broadcast — `BatchDims`'s rest-with-suffix pattern picks
out the last two positions, and `Broadcast` handles the rest:

```python
type MatmulShape[A: Shape, B: Shape] = match A:
    case shape[*ABatch, M, K]: match B:
        case shape[*BBatch, K, N]: shape[*Broadcast[ABatch, BBatch], M, N]
        case _: error   # inner dimensions don't match, or B has fewer than 2 dimensions
    case _: error   # A has fewer than 2 dimensions
```
Reusing `K` as the pattern in `B`'s case, rather than capturing a
fresh name and comparing it afterward, is the exact-match rule doing
the dimension check and the destructuring in the same step.

## Reshape validity

Reshaping is valid only when the total element count is unchanged —
the one property in this whole library that's a genuine computation,
not a structural comparison. `Product` folds
[literal multiplication](type-operations.md#arithmetic-on-literal-types)
over a shape; `Reshape` computes it once for each side and compares:

```python
type Product[S: Shape] = match S:
    case shape[]: 1
    case shape[Head, *Rest]: Head * Product[Rest]

type Reshape[S: Shape, NewShape: Shape] = match Product[S]:
    case Total: match Product[NewShape]:
        case Total: NewShape
        case _: error   # element count doesn't match
```
Concatenation along an axis needs the same kind of computed value —
the sizes of the concatenated axis add, and every other axis has to
match exactly, which the exact-match rule gets for free by reusing the
already-bound `Rest`:

```python
type ConcatAxis0[A: Shape, B: Shape] = match A:
    case shape[AHead, *Rest]: match B:
        case shape[BHead, *Rest]: shape[AHead + BHead, *Rest]
```

## Still open

Stacking `N` arrays along a new leading axis needs `N` itself as a
literal type — the count of however many arrays were passed, not a
dimension already written down anywhere. Nothing here reflects a
gathered `***` argument count as a literal, so a `stack` built this
way would need the caller to supply `N` explicitly rather than infer
it from how many arguments they wrote.

Dispatching a different implementation by rank or shape, rather than
by an argument's runtime class, is also unresolved:
[multiple dispatch](dispatch.md) resolves on runtime class, and shape
is ordinarily a phantom type parameter, not part of it.
