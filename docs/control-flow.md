# Control flow and statements

## Explicit boolean tests

Boolean tests require `bool` or explicit truth behavior. This applies to
`if`, `while`, and other conditional control-flow positions.

## Identity and instance checks

Python's dominant runtime check is `isinstance(x, T)`; plain `is` for
object identity is rare outside `is None`. Lucid swaps the two: `is`
and `is not` become instance checks, and identity moves to `===` and
`!==`, borrowed from JavaScript.

| Lucid | Meaning |
| --- | --- |
| `x is T` | `isinstance(x, T)` |
| `x is not T` | `not isinstance(x, T)` |
| `x === y` | identity |
| `x !== y` | not identity |

A right-hand side naming a class, trait, or union alias is a
type — the swap above applies. A right-hand side whose own static type
is an *instance*, not a class, keeps plain identity instead: `none` is
the ordinary case, so `x is none` and `x is not none` read exactly
as they always have. The same holds for any other singleton value.

A test whose tested type shares no value with the subject's own type can
never hold. Lucid rejects it instead of leaving a dead branch, or a
mistyped name, to be found at runtime:

```python
def f(x: none, w: Widget):
    if x is int: ...   # error: none and int share no value
    if w is str: ...   # error: Widget and str share no value
```
The type tested is the subject's own narrowed type, which is often
sharper than its declaration — [a constructor call infers as final A](types.md), not plain `A`, so the check catches a subclass test
that can never hold too, not only an unrelated type:

```python
class Dog(Animal): ...

def g():
    a = Animal()
    if a is Dog: ...   # error: an exactly-Animal value is never a Dog
```
`==` is unchanged: it still resolves through [Multiple dispatch](dispatch.md), exactly as before. Only `is`/`is not` and
`===`/`!==` change meaning.

## Explicit iteration

`for` loops require an explicitly iterable value. A type is iterable only if
it implements or inherits from `Iterable`.

Advancing an iterator returns a result, the same way any other recoverable
outcome does (see [Errors: results and exceptions](#errors-results-and-exceptions)): either the next
value, or a sentinel meaning there isn't one. Python signals this by
raising `StopIteration` — using an exception for the single most
ordinary outcome an iterator has, not an exceptional one. Lucid's iterators
return a value instead:

```python
import iteration

type IterResult[T] = T | Literal[iteration.done]

trait Iterator[+T]:
    def __next__(self) -> IterResult[T]
```
`iteration.done` is a singleton value, not a class — the same shape as
`none`, just not common enough to earn `none`'s bare-word exemption in
type position, so it is written through `Literal` (see
[Literal types](types.md)) instead of getting a type invented to
represent it. It matches like any other case:

```python
match cursor.__next__():
    case Literal[iteration.done]:
        ...
    case _:
        ...
```
`iteration.done` lives in the `iteration` module rather than being a
builtin the way `none` is: `none` is a genuinely universal absence of a
value, needed everywhere, while `iteration.done` only matters to code
that implements or consumes the iterator protocol directly — a small
enough audience that an explicit import is the right cost, not a builtin
every program pays for.

It needs no bespoke definition either — it is an ordinary
`Sentinel`, the same well-identified singleton any other module can
reach for (see [Name-captured identifiers](construction.md)):

```python
done = Sentinel()
```
## Fresh loop bindings

Python's `for` loop reuses one binding across every iteration: the loop
target is a single variable, reassigned each time around, not a fresh one
per iteration. A closure created inside the loop body captures that same
variable, not its value at the moment of capture, so every closure ends up
seeing whatever the loop left it at when the loop finished, not the value
it appeared to capture:

```python
fns = []
for i in [1, 2, 3]:
    fns.append(def(): print(i))

for f in fns:
    f()   # 3 3 3 in Python -- every closure shares the one binding
```
Lucid gives each iteration its own binding instead, following
`basedpython`: a closure created during one iteration keeps that
iteration's value, unaffected by any that follow.

```python
fns[0]()   # 1
fns[1]()   # 2
fns[2]()   # 3
```
This is the same fix JavaScript's `let` made to its own `var`-based
loops, for the same reason: a variable a reader expects to be scoped to
one iteration should behave that way, not leak its final value into every
closure that captured it. The rule applies wherever a loop introduces a
binding, comprehension targets included.

## No loop `else`

Python loop `else` clauses run when a loop finishes without `break`. Lucid
removes loop `else`.

## `if_broken` loop clauses

Lucid adds an optional `if_broken` clause for loops. `if_broken` is a
keyword. The `if_broken` suite runs if the loop exits by `break`. Ordinary
fall-through code handles the case where a loop exits normally without
`break`.

```python
def find_match(items: Iterable[Item]) -> Item | none:
    for item in items:
        if is_match(item):
            found = item
            break
    if_broken:
        return found
    return none
```
## Nested `if_broken` clauses

In nested loops, an `if_broken` clause belongs to the loop immediately before
it. A `break` inside an inner loop can therefore be handled by the inner
loop's `if_broken` clause, and that clause can choose to break the outer loop:

```python
for row in rows:
    while has_more(row):
        if matches(row.current):
            break
    if_broken:
        break
```
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
[sealed class](classes.md) restricts its direct subclasses to the file
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
covered in [Destructuring with match](destructuring.md), the same rule `let`
uses outside a `match`.
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

## Errors: results and exceptions

Python collapses two different kinds of failure into one mechanism.
`raise`/`try`/`except` handle both "this call can fail in an
ordinary, expected way" — a missing key, a parse failure — and "something
is broken" — a violated invariant, a bug — and because exceptions carry no
signature, nothing about a function's type distinguishes the two. Java
tried to fix the visibility problem by making exceptions checked —
declared in `throws`, enforced by the compiler — and got the goal right
and the mechanism wrong: checked exceptions do not compose with generics
or lambdas, and one new exception type deep in a call chain forces every
intermediate signature to change, which is why nearly every language
designed since has rejected the mechanism while still agreeing with what
it was reaching for.

Lucid splits the two kinds of failure instead of choosing one mechanism for
both.

### Recoverable errors are ordinary return types

An expected, recoverable failure is just part of what a function returns —
a union, handled the same exhaustive way any other union is:

```python
type ParseResult = Value | ParseError

def parse(text: str) -> ParseResult:
    ...

match parse(text):
    case Value:
        ...
    case ParseError:
        ...
```
This needs no new mechanism: it is [Exhaustive pattern matching](#exhaustive-pattern-matching) applied
to errors, not a separate error-handling feature. The checker already
refuses to let a case go unhandled, which is the guarantee Java's checked
exceptions were reaching for, without the signature-propagation cost —
adding a new failure mode to `ParseResult` is an ordinary union change,
not something every caller up the chain has to redeclare.

### The `?` operator

Matching every fallible call by hand does not scale. Go's version of this
problem is `if err != nil { return nil, err }` repeated after nearly
every call — legible, but the boilerplate outweighs the logic it surrounds.
`?` is sugar for the common case: propagate the error case unchanged, or
unwrap to the value and keep going.

```python
def load(path: str) -> Config | ParseError:
    text = read_file(path)?
    return parse(text)?
```
`text = read_file(path)?` means exactly:

```python
match read_file(path) as outcome:
    case ParseError:
        return outcome
    case _:
        text = outcome
```
`?` requires the enclosing function's own return type to already accept
whatever error type is being propagated — `load`'s return type has to
name `ParseError` for either `?` above to be legal, the same
requirement Rust's `?` places on the enclosing function's error type.

This is where Lucid lands between Python and Rust, and closer to Rust.
Python's exceptions propagate automatically with no mark anywhere in the
source that a given call might fail — reading a function's signature tells
you nothing about what it might raise. Lucid's `?` also propagates
automatically, but every place it happens is a literal character in the
source, visible and searchable, the same as Rust's — nothing is invisible:
the return type says what can go wrong, and each `?` says exactly where
a failure is allowed to end the current function early. That is strictly
more than Python gives you, for less ceremony than Go's manual check.

### Unrecoverable errors keep `raise`

`raise`/`try`/`except`/`finally` stay, narrowed to the other kind
of failure: a broken invariant, a bug, something that should never happen
— Rust's `panic!`, not Rust's `Result`. They are not for everyday,
expected outcomes the way Python's `StopIteration`-driven iteration or
its `KeyError`-then-catch idiom use them.

`raise` stays unchecked — not declared in a function's signature, not
enforced by the checker. That is deliberate, not an oversight: the
visibility Java wanted from checked exceptions is already delivered by
recoverable errors being ordinary return types. Checking the broken-
invariant case too would just be ceremony around something no caller is
meant to routinely handle in the first place.

## Unspecified simple statements

This sketch has not yet specified Lucid's full behavior for `assert`,
`break`, or `continue`. `del` itself is fully settled: it is a
compile-time error on a declared field (see [No del on fields](classes.md)), removed entirely in favor of explicit methods on a
mapping or sequence index (see [No __delitem__](indexing.md)), and
retained for exactly one purpose beyond those — see below.

## `del` ends a name's lifetime early

Python's `del` on a plain name removes it from the local namespace;
using it afterward raises `NameError` at that point, dynamically, the
same way any other undefined-name lookup would. Lucid keeps `del` for
exactly this one purpose — ending one or more local names' lifetimes
before their enclosing scope ends — but checks it statically instead:
using a name after `del` has ended its lifetime is a compile-time
error, the same as using a name that was never bound.

```python
def total(prices: list[float]) -> float:
    result = sum(prices)
    del prices     # prices' lifetime ends here, checked
    return result

def total_bad(prices: list[float]) -> float:
    result = sum(prices)
    del prices
    return result + len(prices)  # error: prices' lifetime already ended
```
A single `del` can end several names at once, comma-separated:

```python
del first, second, third
```
This gives `del` a real purpose distinct from letting a scope end
naturally: telling the checker, and the next reader, exactly where a
large or sensitive value's useful life stops — a large buffer dropped
before the rest of a long function runs, or a credential ended as soon as
it's used — enforced the same way every other name-visibility rule in
[Scope](scope.md) already is, rather than left as
a comment nobody checks.

`pass` keeps its ordinary Python meaning: a statement that does
nothing, standing in wherever the grammar requires a statement and the
author has none to write. That makes it the statement-level counterpart
to `skip`, [an expression-level elision marker](calls.md) — the two
fill the same kind of gap one level apart, and neither replaces the
other.

