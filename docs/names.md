# Binding

## Ordinary binding

Names bind through definitions, imports, assignments, and parameters.
Assignment binds names, updates declared fields, or delegates to explicit
assignment behavior such as setters and item assignment.

Assignment binds one name to one value; it has no way to pull several
named fields out of a class instance in one statement.
[Destructuring](destructuring.md) covers the two forms that do.

## Final local variables

`final` marks an ordinary local variable that can be assigned once and
never reassigned:

```python
final total = 0
total = total + 1  # error: total is final
```
Like a final field, this is a property of the binding, not of whatever
value it holds: a `final` binding to a mutable object still lets that
object be mutated through it; it only rules out pointing the name somewhere
else.

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

## Black-hole assignment with `_`

Python treats `_` as an ordinary name, so a value assigned to it stays
readable — a stray later use (a copy-pasted line, a half-finished rename)
silently reads a discarded value instead of failing:

```python
_ = expensive_setup()
if _:      # Python: silently reads the discarded value
    proceed()
```
Lucid makes `_` a black-hole assignment target: assigning to it discards
the value, and `_` is not an expression, so reading it is a syntax error
rather than a silent bug.

```python
_ = compute()
result, _ = split_pair()
use(_)  # error
```
[Scope](scope.md) covers how a binding's visibility is decided, and
what Lucid removes from Python's own rules for it.
