# For and While

## Explicit iteration

`for` loops require an explicitly iterable value. A type is iterable only if
it implements or inherits from `Iterable`.

Advancing an iterator returns a result, the same way any other recoverable
outcome does (see [Results](results.md)): either the next
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
reach for (see [Name-captured identifiers](call-site-captured-values.md)):

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
## Unspecified simple statements

This sketch has not yet specified Lucid's full behavior for `assert`,
`break`, or `continue`. `del` and `pass` are both fully settled. `del`
is a compile-time error on a declared field (see [No del on fields](classes.md)), removed entirely in favor of explicit methods on a
mapping or sequence index (see [No __delitem__](indexing.md)), and
retained for exactly one purpose beyond those: [ending a local
name's lifetime early](names.md#del-ends-a-names-lifetime-early).
`pass` keeps its ordinary Python meaning: a statement that does
nothing, standing in wherever the grammar requires a statement and the
author has none to write. That makes it the statement-level counterpart
to `skip`, [an expression-level elision marker](calls.md) — the two
fill the same kind of gap one level apart, and neither replaces the
other.
