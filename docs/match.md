# Match

## Destructuring with `match`

`case Type(a, b):` destructures the same way [`let`](destructuring.md)
does — the same positional, field-order correspondence — but as one
arm of a `match` instead of a standalone statement, narrowing the
subject to `Type` and pulling its fields out in the same step:

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
next ([Exhaustive pattern matching](#exhaustive-pattern-matching));
this is only the destructuring half, which needs nothing from that
section to make sense on its own.

## Exhaustive pattern matching

Python's `match`/`case` cannot check that every case is covered: Python
classes are open, so there is no way to enumerate every shape a value could
have, and a non-exhaustive match with no matching case just does nothing at
runtime. Lucid's recursive union aliases are closed — every alternative is
named in one place — so the same check Rust and Swift already give their
closed enums is possible here, and Lucid does it:

```python
type PyTree[L] = L | list[PyTree[L]] | dict[str, PyTree[L]]

def tree_map(tree: PyTree[Array], f: (Array) -> bool) -> PyTree[bool]:
    match tree:
        case Array:
            return f(tree)
        case list[PyTree[Array]]:
            return [tree_map(item, f) for item in tree]
        case dict[str, PyTree[Array]]:
            return {k: tree_map(v, f) for k, v in tree.items()}
```
The three cases are exactly `PyTree`'s three alternatives, and that is
checked: leaving one out is an error, not a silent no-op the way an
incomplete Python `match` is. If `PyTree` ever grows a fourth
alternative, every `match` over it that lacks a matching case becomes an
error at the point it stops being exhaustive, rather than a bug found later
at runtime.

A union alias is not the only way to close a type. A
[sealed class](class-inheritance.md) restricts its direct subclasses to the file
that declares it, so the checker can enumerate them the same way it
enumerates a union's alternatives — unlike a plain union's alternatives,
sealed subclasses also share a common parent, and can inherit fields and
methods from it:

```python
match shape:
    case Circle:
        ...
    case Rectangle:
        ...
```
needs no `case _:` if `Shape` is sealed with exactly those two direct
subclasses, the same exhaustiveness `PyTree`'s `match` above already
gets from being a closed union.

A bare `case Type:` pattern just narrows — the subject keeps its own
name, narrowed to `Type` for that case, with nothing pulled out of it.
`case Type(a, b):` does both at once — the positional destructuring
covered in [Destructuring with match](#destructuring-with-match), the
same rule `let` uses outside a `match`.
Narrowing by name only works when the subject already is a name, the way
`tree` is above. When it is some other expression — a call, an
attribute access, anything without a name of its own to reuse — `match`
needs one supplied, with `as`:

```python
match parse(text) as outcome:
    case ParseError:
        ...
    case Value:
        ...
```
`as outcome` names the subject itself, for the cases to narrow and refer
to — it does not name a value the `match` produces, since `match`
does not produce one. `as` is required whenever the subject is not
already a bare name, and has nothing to do otherwise: writing
`match tree as t:` to rename an already-bare subject buys nothing that
`match tree:` did not already have.

A bare `_` matches anything, satisfying exhaustiveness for a match over a
type that is not a closed union at all. Matching against a non-closed type
— `object`, a trait, anything without a known, finite set of
alternatives — cannot be checked for exhaustiveness the way `PyTree` can,
and requires an explicit `case _:` for the same reason a Rust `match`
over an integer needs one: there is no finite set of cases to exhaust.

`match` is a statement, not an expression, and each `case` is an
ordinary statement block — any sequence of statements, exactly like the
body of an `if`, a loop, or a function — not a single expression. It is
purely the exhaustive, type-narrowing form of an `if`/`elif` chain,
nothing more. Producing a value from it works exactly the way producing a value
from an `if`/`elif` chain already does: `return`, as above, or an
assignment repeated in every branch. That repetition is a real, known cost,
not an oversight — the alternative was a case body that is secretly only
ever a single implicit-value expression, which nothing else in the language
does, and which is worse than the repetition it would save.

Pattern matching and dispatch solve related problems differently, and the
choice between them is the same one type theory calls the expression
problem. `match` requires every alternative in one place and checks that
nothing is missing; it is the right tool when the whole set of shapes is
closed and known ahead of time. [Dispatch beyond operators](dispatch.md)
builds the same kind of function the opposite way: an open, growing set of
independently-checked cases that anyone can add to later, with no
exhaustiveness check possible, because the set is never closed. A fixed
`PyTree` with two known container shapes is exactly matched to `match`;
a version meant to stay open to third parties (see
[Existential types](generics.md)) is exactly matched to dispatch instead.
