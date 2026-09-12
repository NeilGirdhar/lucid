# Shapes

A numeric array's shape — how many dimensions it has, and how large
each one is — is exactly the kind of fact [Type vocabulary](types.md)
already wants visible at the definition site, the same way mutability
and variance are. `typing.Shape` and `typing.shape[...]` give it a
type, and this covers the operations built on top of it: taking one
apart, combining two, and checking the constraints an operation like
reshape or matrix multiplication actually needs.

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
At runtime, a shape value is an ordinary `!list[int]` — hashable,
immutable, exactly the sequence [Hashable sequences](collections.md#hashable-sequences)
already recommends for this job. `typing.shape[...]` is the type such
a value can be checked against, the same relationship any other type
has to its values; a concrete `![2, 3, 4]` satisfies
`typing.shape[2, 3, 4]` the same way `(x=1, y=2)` satisfies
`(x: int, y: int)`.

Everything from here on lives in the `typing.shape` module; the
`typing.shape.` prefix is dropped from the definitions below the way
`iteration.done`'s own definition, inside the `iteration` module,
never writes `iteration.` either — only a caller from outside needs
the qualified name.

## Shape is a sequence

Because `shape[...]` fixes both its length and its order, indexing,
slicing, concatenation, and equality already mean exactly what they
mean for any other sequence, extended into type position the same way
[Arithmetic on literal types](type-operations.md#arithmetic-on-literal-types)
already extends `+`/`-`/`*` to a pair of `Literal[int]`:

```python
type Get[S: Shape, I: int] = S[I]
type DropAt[S: Shape, I: int] = S[:I] + S[I + 1:]
type InsertAt[S: Shape, I: int, D: int] = S[:I] + shape[D] + S[I:]
type Concat[A: Shape, B: Shape] = A + B
type Reverse[S: Shape] = S[::-1]
```
None of these need to walk the shape position by position: a fixed
index or a fixed slice boundary already says exactly which positions
are wanted, whether the other end is a concrete length or a generic
parameter standing in for one.

Transpose — swapping the last two axes, the case that actually comes
up, as opposed to swapping two arbitrary indices — is the same
slicing, just naming both ends directly instead of walking to them:

```python
type SwapLast2[S: Shape] = S[:-2] + shape[S[-1], S[-2]]
```

## Batch dimensions

The single most common shape pattern in real numeric code is "some
number of leading batch dimensions, then a fixed trailing shape," and
a fixed-length slice from the *right* already answers it:

```python
type BatchDims[S: Shape] = S[:-3]
type TrailingShape[S: Shape] = S[-3:]
```
`S[:-3]` is every position except the fixed trailing three, however
many leading positions there are; `S[-3:]` is exactly those three. A
slice's other boundary is implicit in "everything else," so pulling
out an unbounded batch prefix needs no name or index of its own — only
the fixed end needs a number.

## Broadcasting

Two dimensions are broadcast-compatible if they're equal, or either
one is exactly `1`. [`promote[A, B]`](type-operations.md#promotion)
already established the pattern for a type-level conditional — an
ordinary `if`/`else`, reused in type position the same way `+`/`-`/`*`
already are:

```python
type BroadcastDim[A: int, B: int] =
    A if A == B else
    B if A == 1 else
    A if B == 1 else
    error   # A and B are not broadcast-compatible
```
Broadcasting aligns from the *right*: peel the last position off each
shape, combine those two with `BroadcastDim`, and recurse on what's
left. Once either shape runs out, the other's remaining prefix passes
through unchanged — a shorter shape's missing leading dimensions
behave as if they were `1`, contributing no constraint:

```python
type BroadcastAligned[A: Shape, B: Shape] =
    B if A == shape[] else
    A if B == shape[] else
    BroadcastAligned[A[:-1], B[:-1]] + shape[BroadcastDim[A[-1], B[-1]]]
```
This resolves the same way `Reverse` and `Concat` do: a self-reference
resolved lazily, the same as any other
[recursive type alias](types.md#recursive-type-aliases) — just walking
from the right instead of the left, since that's the end broadcasting
actually aligns on.

## Matrix multiplication's shape rule

Matmul needs the inner dimensions to match exactly and the batch
dimensions — everything before the last two axes — to broadcast:

```python
type MatmulShape[A: Shape, B: Shape] =
    shape[*BroadcastAligned[A[:-2], B[:-2]], A[-2], B[-1]]
    if A[-1] == B[-2] else
    error   # inner dimensions don't match
```
A shape with fewer than two dimensions needs no separate case: `A[-2]`
on a shape that short is already out of range, the same compile-time
bounds error indexing anywhere else out of range already is — this
gets rejected for free, not as a case this alias has to spell out.

## Reshape validity

Reshaping is valid only when the total element count is unchanged —
the one property in this whole library that's a genuine computation,
not a structural comparison. `Product` folds
[literal multiplication](type-operations.md#arithmetic-on-literal-types)
over a shape the same recursive way `BroadcastAligned` folds
`BroadcastDim`:

```python
type Product[S: Shape] = 1 if S == shape[] else S[0] * Product[S[1:]]

type Reshape[S: Shape, NewShape: Shape] =
    NewShape if Product[S] == Product[NewShape] else
    error   # element count doesn't match
```
Concatenation along an existing axis needs the same kind of computed
value — the sizes of the concatenated axis add, and every other axis
has to match exactly, which is now an ordinary slice comparison:

```python
type ConcatAxis0[A: Shape, B: Shape] =
    shape[A[0] + B[0], *A[1:]]
    if A[1:] == B[1:] else
    error   # every axis but the concatenated one must match
```

## Dispatch by shape

Stacking `N` arrays along a new leading axis needs `N` itself as a
literal type—the count of however many arrays were passed, not a dimension
already written down anywhere. The type-level `stack` operation accepts
operand shapes directly, infers that count from the argument list, and checks
ranks and dimensions before constructing the result shape.

Lucid does not dispatch by shape. [Multiple dispatch](dispatch.md) resolves
on runtime class, and shape is a phantom type parameter of an array, not a
separate runtime class. Two dispatch definitions whose parameters differ only
by shape would therefore erase to the same runtime dispatch key, so the
checker rejects them instead of picking one by declaration order.

Shape-specific behavior belongs in ordinary generic functions whose signatures
state the required shape relationship. The checker can then prove the call
valid from the type-level shape operations above, while the generated function
body stays one ordinary implementation.
