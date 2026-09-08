# Calls

Lucid keeps Python's call syntax but tightens a few of its looser corners:
argument elision, argument order, partial application, and one grammar
special-case Python carries for generator expressions.

## `skip` in calls

The `skip` keyword may appear as a positional or keyword argument. A
positional argument that reaches `skip` is omitted. A keyword argument whose
value reaches `skip` is omitted.

```python
print(1, skip, 3, file=skip)
```
means:

```python
print(1, 3)
```
## Positional arguments precede keyword arguments

Python's rule looks simple — positional arguments come before keyword
arguments — but its grammar (PEP 448) treats star-unpacking as an
exception: an unpacked iterable still binds positionally, but is allowed to
appear in the source *after* a keyword argument, as long as nothing
double-star-unpacked has appeared yet. Source order and binding order can
disagree:

```python
def f(a, b, c):
    ...

f(c=3, *[1, 2])   # legal Python: a=1, b=2, c=3
```
`c=3` is written first but binds to the last parameter; `*[1, 2]`,
written last, supplies the first two. The call has to be read back to
front to see what it does.

Lucid closes the exception: positional arguments — plain and
star-unpacked alike — must precede every keyword argument, including
double-star unpacking, with no interleaving permitted.

```python
f(*[1, 2], c=3)   # fine
f(c=3, *[1, 2])   # error: positional argument follows keyword argument
```
Within the positional section, plain arguments and `*`-unpacking can mix
freely in any relative order, since both bind purely by position, in the
order written. The same holds for named arguments and `**`-unpacking
within the keyword section. The one rule that never bends is between the
two sections: positional, then keyword, always.

## Partial application with `_`

Python's tool for fixing some of a call's arguments ahead of time is
`functools.partial`: `partial(score, weights)` returns a
`functools.partial` object, not an ordinary function, and type checkers
have long struggled to treat it as the `Callable` it behaves like.

Lucid reuses `_` for this — the same "unspecified" marker already used
for black-hole assignment (see [Ordinary binding](names.md))
and the match wildcard (see
[Exhaustive pattern matching](control-flow.md)), now in a call's
argument list. An argument position filled with `_` is not a value; it
leaves that position open, and the call itself becomes a new callable
awaiting whatever positions were left unfilled:

```python
sorted(items, key=score(weights, _))
```
`score(weights, _)` is not a call to `score` — it is a value of type
`(Item) -> float`, built without calling `score` at all, ready to be
called once `sorted` supplies the missing argument itself. No import,
no wrapper object, no separate type to teach a checker about.

Multiple holes fill left to right, matching the order they appear:

```python
def combine(a: A, b: B, c: C) -> R:
    ...

combine(_, y, _)   # (A, C) -> R
```
A keyword hole stays keyword in the result:

```python
combine(x, c=_)    # (C) -> R
```
The type of a partial application is ordinary generic inference, not a
special case: given `f: (A, B) -> R`, `f(x, _)` has type
`(B) -> R`, the same way substituting one type parameter of any other
generic leaves the rest.

## Anonymous functions

Python's `lambda` is a second, narrower construct for the same idea a
`def` already covers, with its own separate keyword borrowed from
lambda calculus. Lucid has no `lambda`. An anonymous function is a
`def` with no name, written as a single expression, always, whose value
it returns:

```python
add = def(x: int, y: int): x + y
```
Parameter types can be omitted when the position already supplies a
function type to check against — the same ordinary generic inference
partial application above already relies on, not a new mechanism, and
never inference from how the body uses them:

```python
add: (int, int) -> int = def(x, y): x + y           # x, y: int, from add's own annotation
cache.get_or_put("ada", def(): load_user("ada"))    # (): User, from get_or_put's signature
```
With nothing to check against, types are written out, same as any named
`def`:

```python
f = def(x, y): x + y            # error: no expected type to infer x, y from
f = def(x: int, y: int): x + y  # fine
```
Zero parameters can drop the empty `()` entirely — there is nothing to
write between `def` and `:` when there are no parameters to name:

```python
log.debug(def: f"Some string {blah()}")
```
Writing `def:` at the call site is visible, not a hidden cost: a
reader sees immediately that the argument may not run, and a linter can
insert it automatically wherever a parameter's declared type calls for
one. A parameter that is sometimes cheap enough to build eagerly and
sometimes not accepts either shape directly, `str | () -> str`,
rather than forcing every caller through the deferred form even when
there is nothing expensive to defer:

```python
def debug(self, message: str | () -> str) -> none:
    if self.level <= Level.debug:
        match message:
            case str:
                self.emit(message)
            case _:
                self.emit(message())

log.debug("starting up")                      # cheap: plain str
log.debug(def: f"state: {expensive_dump()}")   # expensive: deferred
```
The same shape fits any check whose message is sometimes free and
sometimes not — a `precondition`-style helper takes `str | () ->
str` for exactly the same reason `debug` does:

```python
def precondition(ok: bool, message: str | () -> str) -> none:
    if not ok:
        match message:
            case str:
                raise AssertionError(message)
            case _:
                raise AssertionError(message())

precondition(x > 0, "x must be positive")
precondition(x > 0, def: f"x must be positive, got {expensive_repr(x)}")
```
An anonymous `def` has no block form and no `return` — one expression
is the whole body, full stop. Anything that needs more than one statement
needs a name. This is the same discipline
[Exhaustive pattern matching](control-flow.md) already enforces for
`match`: a block that sometimes doubles as a value, depending on what
its last line happens to be, is exactly the ambiguity Lucid avoids
everywhere else, and an anonymous `def` with a block body would be that
same ambiguity one level up. Naming it removes the ambiguity instead of
special-casing around it — and loses nothing capability-wise, since a
named `def` nested inside another function closes over its enclosing
scope exactly as well as an inline one would:

```python
def make_handler(threshold: int) -> (int) -> bool:
    def check(x: int) -> bool:
        if x > threshold:
            log(f"exceeded: {x}")
        return x > threshold
    return check
```
## Generator call expansion

Python's grammar special-cases exactly this shape: a bare generator
expression is allowed as a call's sole argument, borrowing the call's own
parentheses as the generator expression's parentheses. It is not a
consequence of how expressions normally work in a call — it is a dedicated
grammar production for this one case, and it is fragile in a way that shows
the special-casing: `sum(x for x in items)` works, but adding a second
argument breaks it, because now there are two arguments and the borrowed
parentheses no longer apply. `sum(x for x in items, start=0)` is a syntax
error; the generator expression needs its own parentheses back —
`sum((x for x in items), start=0)`. Whether the parentheses can be omitted
depends on argument count, not on what the expression means.

Lucid needs no special case: a generator expression written as a call
argument is treated the same as any other expression written there, and
argument expansion follows regardless of how many other arguments are
present.

```python
f(x for x in [x_1, x_2, x_3])
```
means:

```python
f(x_1, x_2, x_3)
```
To pass an actual generator object, parenthesize the generator expression:

```python
f((x for x in items))
```
