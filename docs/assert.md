# Assert

Python's `assert` takes a bare, comma-separated condition and message,
with no parentheses: `assert x == y, "message"`. Parentheses are
ordinary grouping syntax almost everywhere else in the language, so
wrapping that pair in them looks like a harmless formatting choice:

```python
assert (x == y, "x and y should match")   # always true in Python
```
It silently builds a 2-tuple instead of calling `assert` with two
arguments. A non-empty tuple is always truthy, so the assertion can
never fail — a mistake common enough that Python's own linters watch
for it specifically.

Lucid's `assert` requires the parentheses instead of forbidding them:
`assert(condition)`, or `assert(condition, message)` for an assertion
with an explanation. There is no bare, comma-separated form to reach
for by mistake, so there is nothing to get right or wrong about
whether to parenthesize — one spelling, the same principle
[No tuple or namedtuple type](collections.md) already applies to
records.

```python
assert(x == y)
assert(x == y, "x and y should match")
```
A failed assertion raises `AssertionError`, an ordinary exception (see
[Exceptions](exceptions.md)) — an assertion is exactly the kind of
broken-invariant failure `raise` is for, not a recoverable one.

The message accepts `str | () -> str`, the same shape
[a `precondition`-style helper](calls.md#anonymous-functions) already
takes and for the same reason: building an expensive description costs
nothing on the common path where the assertion holds.

```python
assert(x > 0, f"x must be positive, got {expensive_repr(x)}")        # always builds the message
assert(x > 0, def: f"x must be positive, got {expensive_repr(x)}")   # only builds it if x <= 0
```
