# Gather

A function names its own catch-all with a leading `***`, declaring it
directly as an `Arguments` value rather than as two separate parameters:

```python
def query(path: Path, ***rest: Arguments[str, dict[str, str]]) -> Response:
    ...
```
`***` can mark at most one parameter, and it must be last — there is
nothing left to catch after it. A bare `*` with no name still marks the
start of a keyword-only zone for ordinary named parameters, independent of
`***`: a function is never forced to route a keyword-only parameter
through the bundle just to give it one. `query` needs only `Arguments`
here, since `path` is already an ordinary, separately named parameter —
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

[Spread](spread.md) covers sending a gathered bundle back out at
another call site.
