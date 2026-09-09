# Arguments

A callable's parameter shape can be written as a type in its own right
— see [Anonymous class](anonymous-class.md). The leftover arguments a
signature doesn't name explicitly still need somewhere typed to go.

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

Lucid gathers leftover arguments into one typed class instead of two
untyped catch-alls:

```python
class Arguments[Y, Z: dict[str, object]]:
    vpargs: list[Y]
    kwargs: Z
```
`vpargs` holds the leftover positional arguments, homogeneously typed;
`kwargs` holds the leftover keyword arguments, typed as a full shape —
`{str: str}`, or a TypedDict shape when some of those keywords are
individually named — rather than a bare per-value type. Writing
`kwargs: str` would say each value is a `str`, not that `kwargs`
itself is a mapping; the field has to be annotated with what it actually
holds.

`Arguments` on its own is for genuinely unnamed overflow: arguments
beyond anything a function declared, with no name available to give
them, and no fixed prefix left to keep separate. A signature with a
fixed, named prefix to keep separate from that overflow needs
[Parameters](parameters.md) instead, the class `Arguments` alone
doesn't try to be.
