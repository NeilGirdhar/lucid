# Gather

A signature gathers its own leftover arguments one of three ways: split
into typed positional and keyword catch-alls (`*`/`**`), bundled
together into one typed value (`***`), or not gathered at all. Exactly
one applies to a given signature — splitting and bundling both claim
the entire leftover positional-and-keyword overflow, just shaped
differently, so mixing them would ask the same arguments to go two
places.

## Positional and keyword catch-alls

`*name: T` gathers the leftover positional arguments into
`name: list[T]`, homogeneously typed — the spelling Python's `*args`
already uses, but checked: every extra argument must actually be `T`,
never `Any`, the way `*args` stayed for most of Python's history.
`**name: T` gathers the leftover keyword arguments the same way, into
`name: dict[str, T]`. A signature can use either alone or both
together, since each names its own, independent piece of the overflow:

```python
def total(*values: int) -> int:
    return sum(values)

def render(template: str, **fields: str) -> str:
    ...

def zip_with(*values: int, **labels: str) -> dict[str, int]:
    ...
```
A bare `*` with no name after it still only opens the keyword-only
zone for the ordinary named parameters that follow — it never gathers
on its own, the same distinction Python already draws between `*args`
and a bare `*`:

```python
def f(a: int, *, b: int) -> none:  # b is keyword-only; nothing gathered
    ...
```

## Bundled gather

A function that needs to forward its overflow to another call as one
value — the case a decorator's generic forwarding needs, covered in
[Arguments](arguments.md) — names it with a leading `***` instead,
declaring it directly as an `Arguments` value:

```python
def query(path: Path, ***rest: Arguments[str, dict[str, str]]) -> Response:
    ...
```
`***` can mark at most one parameter, and it must be last — there is
nothing left to catch after it. `query` needs only `Arguments` here,
since `path` is already an ordinary, separately named parameter —
there is no fixed prefix left for a `pargs` field to hold.

For an ordinary class (see [Anonymous class](anonymous-class.md) and [Decorators](decorators.md)),
`***name: SomeClass` gathers one-to-one: each of `SomeClass`'s fields
corresponds to exactly one of the function's own remaining parameters, in
whichever positional or keyword zone that class itself declares. When
`SomeClass` is `Arguments`, `Parameters`, or a class that inherits
from either, gathering instead binds by role: `pargs` binds to whatever
fixed prefix remains, the ordinary way, and however many leftover
positional and keyword arguments the caller actually supplied — an
unbounded number — aggregate into `vpargs` and `kwargs`. This is
decided by `SomeClass`'s type, checked statically the same way
`construct` is recognized by the compiler rather than looked up as an
ordinary call ([Factory construction](construction.md)): an unrelated class
that happens to declare a field named `pargs`, `vpargs`, or `kwargs`
still gathers one-to-one, because it isn't an `Arguments`. That's the
one place `Arguments` and `Parameters` aren't just ordinary classes:
the aggregating behavior belongs to those two classes specifically, not to
any class that reuses their field names.

## Split and bundled forms never mix

A signature picks one shape for its overflow, never a combination:

```python
def f(*x: int, ***rest: Arguments[int, dict[str, str]]) -> none:  # error
    ...
```
`*x` already claims every leftover positional argument; `***rest`
would have nothing positional left to gather, and the same conflict
rules out `**name` alongside `***name`, or all three together.

## Naming convention

`args` is reserved for a variable holding a full `Arguments` or
`Parameters` instance — the bundled form always uses it, as
[Decorators](decorators.md)'s `***args: P` does. The split form's
conventional names are the field names its pieces would have inside
that same bundle: `*vpargs: T` for the positional catch-all,
`**kwargs: T` for the keyword one — Python's own name for it,
unchanged. `pargs` never names a bare `*`/`**`/`***` parameter itself;
it names the *fixed* prefix field inside an actual `Parameters` value,
which only a `***` parameter can hold.

[Spread](spread.md) covers sending a gathered bundle back out at
another call site.
