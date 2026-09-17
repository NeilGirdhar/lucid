# Arguments

A callable's parameter shape can be written as a type in its own right
— see [Anonymous class](anonymous-class.md). The leftover arguments a
signature doesn't name explicitly still need somewhere typed to go.

Call arguments already spread two ways: `*expr` distributes a sequence
across positional slots, and `**expr` distributes a mapping across
keyword arguments — the same two operations
[Positional arguments precede keyword arguments](calls.md) assumes throughout its
examples.

Python collects a function's own leftover arguments the same way, but as
two separate, untyped catch-alls: `*args` for the leftover positional
values, `**kwargs` for the leftover keyword values, both essentially
`Any`-typed for most of Python's history. [Gather](gather.md) keeps
that same split for the ordinary case — `*name: T`/`**name: T`, but
checked against `T`, never `Any`.

The split form doesn't help a decorator or a proxy that needs to
forward an *unknown* signature's entire overflow to another call,
though: it still needs to carry `*args` and `**kwargs` as two separate,
unrelated names everywhere they travel, exactly the case Python's own
`ParamSpec` exists to type and still can't fully close ([Decorators](decorators.md)).

Lucid gathers leftover arguments meant for forwarding into one typed
class instead:

```python
class Arguments[Y, Z: ~dict[str, object]]:
    vpargs: list[Y]
    kwargs: Z
```
`vpargs` holds the leftover positional arguments, homogeneously typed;
`kwargs` holds the leftover keyword arguments, typed as a full shape —
`dict[str, str]`, or a TypedDict shape when some of those keywords are
individually named — bounded by `~dict[str, object]` so that covariant
read-only mappings satisfy it rather than invariant `dict` (see
[Mutability](mutability.md)). Writing `kwargs: str` would say each
value is a `str`, not that `kwargs` itself is a mapping; the field has to
be annotated with what it actually holds.

`Arguments` on its own is for genuinely unnamed overflow: arguments
beyond anything a function declared, with no name available to give
them, and no fixed prefix left to keep separate. A signature with a
fixed, named prefix to keep separate from that overflow needs
[Parameters](parameters.md) instead, the class `Arguments` alone
doesn't try to be.
