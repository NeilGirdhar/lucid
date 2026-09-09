# Match types

A recursive alias like `PyTree` picks its shape by union — every
alternative is listed once, up front. Sometimes the type to produce
depends on the *structure* of another type instead: whether a list is
nested another level deeper, or already down to its leaf. A match type
is a `type` alias whose right side is `match`, reusing the same
`match`/`case` grammar [Exhaustive pattern matching](control-flow.md)
already has, computing a type from a type instead of a value from a
value:

```python
type Elem[T] = match T:
    case list[Inner]: Elem[Inner]
    case Leaf: Leaf
```
`Elem[list[list[int]]]` is `int`: each case peels off one layer of
nesting and recurses, until what is left is not a `list` at all.

Value-level `match` is a statement, deliberately without a case-body-
as-implicit-expression form, because that would make an ordinary
statement block secretly double as a value some of the time. A match
type has no such ambiguity to guard against: it lives entirely in the
type-expression grammar [Type expressions](types.md#type-expressions)
already describes, and every case's only job, ever, is to produce one
type — so each case is a bare type expression, no block form needed.
Pattern capture follows the same convention Python's own structural
pattern matching already uses: a name that already names something is an
exact match (`case list[Inner]:` matches the shape `list` exactly),
and a name that doesn't is a fresh capture, bound to whatever the
subject actually was for use on the right (`Inner`, or `Leaf` for
whatever falls through to the second case).

A captured name can itself be a union — a subject typed `A | B` makes
any fresh capture over it a capture of `A | B` as a whole — and a
fresh-capture case runs once per member of a union subject, unioning the
results, the same distribution TypeScript's own conditional types
already do. This is what lets a recursive match type work through a
branching structure instead of getting stuck treating the whole union as
one opaque type: [Promotion](dispatch.md), later in the reading
order, puts exactly this to use for a type that branches in more than
one direction. Combined with `Never` already
being union's identity element — a type with no values contributes
nothing to a union, so `Never | X` is just `X`, true of any bottom
type in any type system that has one — distribution gives an
existential test for free:
if a fresh-capture case produces `Never` down every branch, the
distributed result is `Never`; if it produces something else down even
one branch, that survives, since every `Never` alongside it disappears.
