# Match types

A recursive alias like `PyTree` picks its shape by union — every
alternative is listed once, up front. Sometimes the type to produce
depends on the *structure* of another type instead: how many
positions it has, or what one specific position holds. A match type is
a `type` alias whose right side is `match`, reusing the same
`match`/`case` grammar [Exhaustive pattern matching](match.md) already
has, computing a type from a type instead of a value from a value.

A *shape* — a fixed-length sequence of dimension sizes, spelled
`typing.shape[2, 3, 4]` — is exactly the kind of structure a match type
picks apart. [Shapes](shape.md) covers the type in full; this only
needs its outline to show how the grammar works:

```python
type Rank[S: typing.Shape] = match S:
    case typing.shape[]: 0
    case typing.shape[_, *Rest]: 1 + Rank[Rest]
```
`Rank[typing.shape[2, 3, 4]]` is `Literal[3]`: each case peels one
position off the front and recurses — the `_` in the second case
discards the position itself, keeping only the count — until nothing
is left and the base case returns `0`. `1 + Rank[Rest]` is
[ordinary literal arithmetic](type-operations.md#arithmetic-on-literal-types)
at the type level, resolving to a concrete `Literal[N]` the same way
any other expression resolves to a value.

Value-level `match` is a statement, deliberately without a case-body-
as-implicit-expression form, because that would make an ordinary
statement block secretly double as a value some of the time. A match
type has no such ambiguity to guard against: it lives entirely in the
type-expression grammar [Type expressions](types.md#type-expressions)
already describes, and every case's only job, ever, is to produce one
type — so each case is a bare type expression, no block form needed.

Pattern capture follows the same convention Python's own structural
pattern matching already uses: a name that already names something is
an exact match, and a name that doesn't is a fresh capture, bound to
whatever the subject actually was for use on the right. `typing.shape`
itself is an exact match on the outer structure, the same way `list`
is in `case list[Inner]:`; a literal position inside it, like the `1`
below, is exactly as exact — it matches only a shape whose first
dimension actually is `1`:

```python
type SqueezeLeading[S: typing.Shape] = match S:
    case typing.shape[1, *Rest]: Rest
    case Same: Same
```
`Rest` and `Same` are fresh captures; `1` is not.

A captured name can itself be a union — a subject typed `A | B` makes
any fresh capture over it a capture of `A | B` as a whole — and a
fresh-capture case runs once per member of a union subject, unioning
the results, the same distribution TypeScript's own conditional types
already do. This is what lets a recursive match type work through a
branching structure instead of getting stuck treating the whole union
as one opaque type: [Promotion](dispatch.md#promotion), later in the
reading order, puts exactly this to use for a type that branches in
more than one direction. Combined with `Never` already being union's
identity element — a type with no values contributes nothing to a
union, so `Never | X` is just `X`, true of any bottom type in any type
system that has one — distribution gives an existential test for free:
if a fresh-capture case produces `Never` down every branch, the
distributed result is `Never`; if it produces something else down even
one branch, that survives, since every `Never` alongside it disappears.
