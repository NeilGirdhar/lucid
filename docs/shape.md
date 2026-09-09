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

## Broadcasting and other runtime checks

Getting a position, dropping one, concatenating, reversing — every
operation above is purely structural: the checker can answer it from
the shapes' literal lengths and positions alone, the same way it
resolves any other type expression. Broadcasting, matrix
multiplication's shape rule, reshape validity, and concatenation along
an existing axis are a different kind of question: whether two
*already-known* shapes are compatible, which is exactly the sort of
fact [Results](results.md) already treats as an ordinary, recoverable
return value rather than something the type system has to prove ahead
of time.

Two dimensions are broadcast-compatible if they're equal, or either
one is exactly `1`:

```python
def broadcast_dim(a: int, b: int) -> int | none:
    if a == b:
        return a
    if a == 1:
        return b
    if b == 1:
        return a
    return none
```
Broadcasting aligns from the *right*, so `broadcast` reverses both
shapes, aligns from the front, and reverses the result back:

```python
def broadcast(a: !list[int], b: !list[int]) -> !list[int] | none:
    ra, rb = a[::-1], b[::-1]
    dims: !list[int] = ![]
    for i in range(max(len(ra), len(rb))):
        da = ra[i] if i < len(ra) else 1
        db = rb[i] if i < len(rb) else 1
        match broadcast_dim(da, db):
            case none:
                return none
            case d:
                dims = ![d, *dims]
    return dims
```
Matmul needs the inner dimensions to match exactly and the batch
dimensions — everything before the last two axes — to broadcast:

```python
def matmul_shape(a: !list[int], b: !list[int]) -> !list[int] | none:
    if len(a) < 2 or len(b) < 2:
        return none
    if a[-1] != b[-2]:
        return none
    match broadcast(a[:-2], b[:-2]):
        case none:
            return none
        case batch:
            return batch + ![a[-2], b[-1]]
```
Reshaping is valid only when the total element count is unchanged:

```python
def product(s: !list[int]) -> int:
    total = 1
    for dim in s:
        total *= dim
    return total

def reshape_is_valid(s: !list[int], new_shape: !list[int]) -> bool:
    return product(s) == product(new_shape)
```
Concatenation along an existing axis needs the same kind of check —
the sizes of the concatenated axis add, and every other axis has to
match exactly:

```python
def concat_axis0(a: !list[int], b: !list[int]) -> !list[int] | none:
    if a[1:] != b[1:]:
        return none
    return ![a[0] + b[0], *a[1:]]
```
Each of these returns `none` for an incompatible pair rather than
raising, the same trade [Results](results.md) makes for every other
expected, recoverable failure — a caller checks the result with an
ordinary `match`, the same way any other fallible call is checked.

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
