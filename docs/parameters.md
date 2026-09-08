# Parameters and arguments

A callable's parameter shape can be written as a type in its own right, and
the leftover arguments a signature doesn't name explicitly still need
somewhere typed to go. This covers both: the anonymous class shape that
fills a `Callable`'s parameter position, and `Arguments`/`Parameters`,
the two classes that gather and spread whatever a signature leaves open.

## Anonymous class

[Anonymous record shapes](collections.md) already give a value shape
without declaring a name for it: `(x: int, y: int)`. A callable's own
parameter list uses the same grammar, on the left of a
[function type's](types.md) `->`, whenever a function has no
variadic zoning of its own:

```python
(str, bool) -> R
```
— the type of a function like `greet(name: str, loud: bool)`.

A `/` marks the end of a positional-only zone; a bare `*` marks the
start of a keyword-only one — the same two markers an ordinary parameter
list already uses, and both stay within this same, plain shape:

```python
(c: int, /, a: int, *, b: int)
```
- `c: int, /` — positional-only.
- `a: int` — ordinary: callable by position or by keyword.
- `*` — the keyword-only zone begins.
- `b: int` — keyword-only.

Every field here has a name, because every field here is one fixed,
individually addressable slot — whether or not a caller can ever use
that name. An ordinary or keyword-only field needs one because the
checker has to know it to verify a keyword call. A positional-only field
needs one for a different reason: callers can never use it, but the name
is what marks it as *one* slot. The order — positional-only, then
ordinary, then keyword-only — isn't a style rule: nothing can be
keyword-only until the positional zone is finished.

A signature with an unbounded, unnamed tail — the case a decorator's
generic forwarding needs, or a genuine overflow catch-all — extends this
grammar further; see
[Gathering arguments with Arguments and Parameters](#gathering-arguments-with-arguments-and-parameters).

## Gathering arguments with `Arguments` and `Parameters`

Call arguments already spread two ways: `*expr` distributes a sequence
across positional slots, and `**expr` distributes a mapping across
keyword arguments — the same two operations
[Positional arguments precede keyword arguments](calls.md) assumes throughout its
examples.

Python collects a function's own leftover arguments the same way, but
splits them into two separate, untyped catch-alls: `*args` for the
leftover positional values, `**kwargs` for the leftover keyword values.
The keyword half stayed essentially `Any`-typed for most of Python's
history, and forwarding both together to another call — the entire point
of a decorator or a proxy — means carrying two separate names everywhere
they travel.

Lucid gathers leftover arguments into one typed value instead of two
untyped ones, and splits that into two classes depending on whether
there's a fixed, named prefix to keep separate from the open overflow:

```python
class Arguments[Y, Z: dict[str, object]]:
    vpargs: list[Y]
    kwargs: Z

class Parameters[X, Y, Z: dict[str, object]](Arguments[Y, Z]):
    pargs: X
```
Neither declaration needs a bespoke type-system primitive — both are
ordinary generic classes, `Parameters` inheriting from `Arguments` the
ordinary way (see
[Class inheritance and runtime hooks](classes.md)).
`vpargs` holds the leftover positional arguments, homogeneously typed;
`kwargs` holds the leftover keyword arguments, typed as a full shape —
`{str: str}`, or a TypedDict shape when some of those keywords are
individually named — rather than a bare per-value type. Writing
`kwargs: str` would say each value is a `str`, not that `kwargs`
itself is a mapping; the field has to be annotated with what it actually
holds. `pargs` holds whatever fixed, possibly zoned prefix a signature
has, typed as an [Anonymous class](#anonymous-class) itself when it needs its own
positional-only or ordinary zoning.

`Arguments` on its own is for genuinely unnamed overflow: arguments
beyond anything a function declared, with no name available to give them,
and no fixed prefix left to keep separate. `Parameters` is for
describing a signature whole — the case a decorator's generic forwarding
needs, where there's no way to declare the fixed part as ordinary,
separately named parameters up front, because the wrapper doesn't know
what they will be (see [Decorators](decorators.md)). `Parameters` is usable as a
bound the same way `dict` or `Functor` are, and a plain nominal
class's unzoned field shape satisfies it as the simplest case.

A signature's full shape, open-ended content included, can be written
inline by extending [Anonymous class](#anonymous-class)'s grammar with its two remaining
zones: a bare, unnamed type followed by `...` for the variadic
positional tail, and `_: type, ...` for the variadic keyword one, after
the fixed prefix and its keyword-only zone respectively — the same
positional-before-keyword order every zone already follows:

```python
(c: int, /, a: int, int, ..., *, b: int, _: int, ...)
```
- `c: int, /, a: int` — the fixed, possibly zoned prefix:
  [Anonymous class](#anonymous-class), exactly as written there.
- `int, ...` — variadic positional: zero or more further `int`\ s,
  after every fixed position — nothing fixed can follow it, since it
  consumes every remaining positional slot. The trailing `...` is the
  same marker that opens a TypedDict shape
  ([TypedDict shapes in type position](collections.md)), here meaning
  "and more of the type just written," not "anything."
- `*, b: int` — the keyword-only zone, [Anonymous class](#anonymous-class) again.
- `_: int, ...` — variadic keyword: zero or more further keyword
  arguments, each `int`. `_` marks the slot as nameless the same way
  it already does for black-hole assignment
  ([Ordinary binding](names.md)) — the key isn't fixed,
  only the value's type is.

This is inline sugar for a direct `Parameters` instantiation:

```python
Parameters[(c: int, /, a: int), int, {"b": int, _: int, ...}]
```
Only the two variadic markers — a bare, unnamed run and `_: type,
...` — trigger this desugaring. `/` and a named keyword-only zone
don't: they stay inside the fixed prefix, an ordinary [Anonymous class](#anonymous-class)
shape in its own right, which is why `pargs`'s own type can have a
`/` in it without becoming another `Parameters` — a fixed, zoned but
non-variadic prefix needs no aggregation, only genuinely open-ended
content does. `query`'s full type follows the same rule, with an empty
prefix, since `path` is already named separately:

```python
(Path, str, ..., *, _: str, ...) -> Response
```
What isn't ordinary about either is how `***` treats `pargs`,
`vpargs`, and `kwargs` specifically, on both the gathering and the
spreading side.

### Gather

A function names its own catch-all with a leading `***`, declaring it
directly as an `Arguments` value rather than as two separate parameters:

```python
def query(path: Path, ***rest: Arguments[str, {str: str}]) -> Response:
    ...
```
`***` can mark at most one parameter, and it must be last — there is
nothing left to catch after it. A bare `*` with no name still marks the
start of a keyword-only zone for ordinary named parameters, independent of
`***`: a function is never forced to route a keyword-only parameter
through the bundle just to give it one. `query` needs only `Arguments`
here, since `path` is already an ordinary, separately named parameter —
there is no fixed prefix left for a `pargs` field to hold.

For an ordinary class (see [Anonymous class](#anonymous-class) and [Decorators](decorators.md)),
`***name: SomeClass` gathers one-to-one: each of `SomeClass`'s fields
corresponds to exactly one of the function's own remaining parameters, in
whichever positional or keyword zone that class itself declares.
`pargs`, `vpargs`, and `kwargs` are recognized field names that mean
something else: rather than expecting parameters literally named that,
the gather binds `pargs` to whatever fixed prefix remains, one-to-one,
the ordinary way, and aggregates however many leftover positional and
keyword arguments the caller actually supplied — an unbounded number —
into `vpargs` and `kwargs`. That's the one place `Arguments` and
`Parameters` aren't just ordinary classes: the aggregating behavior is
tied specifically to those field names.

### Spread

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
class Arguments[Y, Z: dict[str, object]]:
    vpargs: list[Y]
    kwargs: Z

    def __spread__(self: ~Self) -> ~Parameters[(), Y, Z]:
        return Parameters.from_arguments(self)

class Parameters[X, Y, Z: dict[str, object]](Arguments[Y, Z]):
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


