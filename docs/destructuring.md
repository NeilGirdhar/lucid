# Destructuring

Assignment binds one name to one value; it has no way to pull several
named fields out of a class instance in one statement. Destructuring
pulls several bindings out of one value at once, by position — the
same order a factory call already uses to construct that value. Lucid
has two forms, sharing that one rule: `let` outside a `match`, `case`
inside one.

## Destructuring with `let`

`let` binds a class's fields directly, by position:

```python
let Point(x, y) = origin
```
which binds `x` to `origin.x` and `y` to `origin.y`, the same
positional correspondence `Point(1.0, 2.0)` already uses going the
other way. The names inside the pattern are ordinary new bindings, not
required to match the class's own field names, the same way tuple
unpacking's `a, b = pair` never cared what `pair`'s own names were:

```python
let Point(px, py) = origin   # binds px, py -- Point's own fields stay x, y
```
`let` is already the signal that the left side is a pattern rather
than an assignment target, so the binding itself stays the ordinary
`=` every other binding already uses — no second operator like
basedpython's `:=` is needed to mark it twice.

The pattern has to be one the checker can already prove: `origin`'s
static type must already be (a subtype of) `Point` — `let` has
nothing to fall back to if the pattern turns out not to fit. A value
that could be one of several different variants needs the exhaustive
form covered next, built to handle "this could be any of these" —
`let` is only for "this already is one specific thing, pull it
apart." Anonymous records destructure the same way, by their own
declared order:

```python
let (x, y) = midpoint   # midpoint: (x: int, y: int)
```
The same pattern works anywhere a name would otherwise bind — a `for`
target:

```python
for Point(x, y) in points:
    ...
```
or a parameter:

```python
def distance(Point(x1, y1), Point(x2, y2)) -> float:
    ...
```

## Destructuring with `match`

`case Type(a, b):` destructures the same way `let` does — the same
positional, field-order correspondence — but as one arm of a `match`
instead of a standalone statement, narrowing the subject to `Type` and
pulling its fields out in the same step:

```python
match shape:
    case Point(x, y):
        ...
```
The difference from `let` is exactly the difference between the two
forms' jobs: `let` requires the pattern to already be proven, with
nothing to fall back to; `match` exists for a subject that could be
any of several variants, and checks that every one is covered.
`match`'s own grammar — the narrowing-only form `case Type:` with
nothing pulled out, exhaustiveness checking, and the rest — is covered
later in this reading order
([Exhaustive pattern matching](control-flow.md)); this is only the
destructuring half, which needs nothing from that section to make
sense on its own.
