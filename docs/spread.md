# Spread

The same sigil spreads a value back out at a call site by calling a
method every class has — `__spread__` — and using the `Parameters`
instance it returns. Every class gets one generated for free, the same
way every class already gets a generated `__init__` and `replace`
factory ([Factory construction](construction.md)). By default,
`__spread__` wraps the instance as its own fixed prefix, with nothing
variadic:

```python
def __spread__(self: ~Self) -> ~Parameters[Self, Never, {:}]:
    return Parameters((), {:}, self)
```
which is why an ordinary class like [Decorators](decorators.md)'s `(name: str, loud:
bool)` spreads as one argument per field: `pargs` is the instance
itself, `vpargs`/`kwargs` are empty.

`Arguments` and `Parameters` override the default instead of using
it:

```python
class Arguments[Y, Z: ~dict[str, object]]:
    vpargs: list[Y]
    kwargs: Z

    def __spread__(self: ~Self) -> ~Parameters[(), Y, Z]:
        return Parameters.from_arguments(self)

class Parameters[X, Y, Z: ~dict[str, object]](Arguments[Y, Z]):
    pargs: X

    factory from_arguments(cls, args: ~Arguments[Y, Z]) -> Parameters[(), Y, Z]:
        return construct(args.vpargs, args.kwargs, ())

    def __spread__(self: ~Self) -> ~Self:
        return self
```
`Arguments` converts itself into the `Parameters` it structurally is
— the same shape, with an empty `pargs` — reusing
`Parameters.from_arguments` rather than duplicating the logic.
`from_arguments` is a named factory, so `construct` assigns
positionally in `Parameters`'s own field order: inherited fields first
(`vpargs`, `kwargs`, from `Arguments`), then `pargs`, the field
`Parameters` adds — the same top-to-bottom order the class hierarchy
declares them in. `pargs=()` is fixed by this factory specifically, the
same way `Point.origin()` ([Factory construction](construction.md)) fixes `x=0.0, y=0.0` while still
constructing the exact class generically over whatever else varies.
`Parameters` is already the target shape its own `__spread__` needs
to produce, so it just returns itself.

Nothing about either class is special at the type level anymore — what's
special is that they implement `__spread__` differently from the
generated default, exactly the way any class can override any other
method. Once `***value` has `value.__spread__()`'s result, spreading
follows the zones in reverse: `pargs`'s own fields spread first, in
their own order — positional-only, then ordinary — then `vpargs`'s
elements, positionally, then `kwargs`'s entries, as keywords. Never a
plain `**` mapping-spread on its own: a class with any positional-only
or variadic-positional field could never be satisfied that way.

```python
query(***rest)
```
means exactly:

```python
query(*rest.vpargs, **rest.kwargs)
```
— no `pargs` here, since a plain `Arguments` value doesn't have one.
A `Parameters` value spreads the same way with `pargs` added at the
front: for a signature matching
`(c: int, /, a: int, int, ..., *, b: int, _: int, ...)`, with
`pargs = (c=1, a=2)`, `vpargs = [3, 4]`, and
`kwargs = {"b": 5, "x": 6}`,

```python
f(***params)
```
means:

```python
f(1, 2, 3, 4, b=5, x=6)
```
This degenerates to just `pargs`'s own fields whenever
`vpargs`/`kwargs` are empty — exactly [Decorators](decorators.md)'s `f(***args)`
for a function with no variadic tail of its own.

Neither field becomes a keyword argument in its own name — `pargs`'s
fields, `vpargs`'s elements, and `kwargs`'s entries spread as
themselves, not as arguments literally called `pargs`, `vpargs`, or
`kwargs`. Gathering leftover arguments into a parameter and spreading
them back out to forward them are both a single token, using the same
`*` and `**` spreads every other call already relies on, just
performed together and aimed at the recognized fields.

Because `pargs`/`vpargs`/`kwargs` alone read too close to a
variable that holds an entire bundle, the pieces are kept distinct by
convention: those three name the fields, and `args` is reserved for a
variable that holds a full `Arguments` or `Parameters` instance —
exactly how [Decorators](decorators.md) uses it.
