# Gather

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

For an ordinary class (see [Anonymous class](anonymous-class.md) and [Decorators](decorators.md)),
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

[Spread](spread.md) covers sending a gathered bundle back out at
another call site.
