# Parameters

[Arguments](arguments.md) gathers a function's leftover arguments when
there's no fixed, named prefix to keep separate from the open overflow.
`Parameters` extends it with exactly that prefix:

```python
class Parameters[X, Y, Z: ~dict[str, object]](Arguments[Y, Z]):
    pargs: X
```
Neither declaration needs a bespoke type-system primitive — both are
ordinary generic classes, `Parameters` inheriting from `Arguments` the
ordinary way (see [Class inheritance](class-inheritance.md)). `pargs`
holds whatever fixed, possibly zoned prefix a signature has, typed as
an [Anonymous class](anonymous-class.md) itself when it needs its own
positional-only or ordinary zoning.

`Parameters` is for describing a signature whole — the case a
decorator's generic forwarding needs, where there's no way to declare
the fixed part as ordinary, separately named parameters up front,
because the wrapper doesn't know what they will be (see
[Decorators](decorators.md)). `Parameters` is usable as a bound the
same way `dict` or `Functor` are, and a plain nominal class's unzoned
field shape satisfies it as the simplest case.

A signature's full shape, open-ended content included, can be written
inline by extending [Anonymous class](anonymous-class.md)'s grammar with its two remaining
zones: a bare, unnamed type followed by `...` for the variadic
positional tail, and `_: type, ...` for the variadic keyword one, after
the fixed prefix and its keyword-only zone respectively — the same
positional-before-keyword order every zone already follows:

```python
(c: int, /, a: int, int, ..., *, b: int, _: int, ...)
```
- `c: int, /, a: int` — the fixed, possibly zoned prefix:
  [Anonymous class](anonymous-class.md), exactly as written there.
- `int, ...` — variadic positional: zero or more further `int`\ s,
  after every fixed position — nothing fixed can follow it, since it
  consumes every remaining positional slot. The trailing `...` is the
  same marker that opens a TypedDict shape
  ([TypedDict shapes in type position](types.md#typeddict-shapes-in-type-position)), here meaning
  "and more of the type just written," not "anything."
- `*, b: int` — the keyword-only zone, [Anonymous class](anonymous-class.md) again.
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
don't: they stay inside the fixed prefix, an ordinary [Anonymous class](anonymous-class.md)
shape in its own right, which is why `pargs`'s own type can have a
`/` in it without becoming another `Parameters` — a fixed, zoned but
non-variadic prefix needs no aggregation, only genuinely open-ended
content does. `query`'s full type follows the same rule, with an empty
prefix, since `path` is already named separately:

```python
(Path, str, ..., *, _: str, ...) -> Response
```
What isn't ordinary about either class is how `***` treats `pargs`,
`vpargs`, and `kwargs` specifically, on both the gathering and the
spreading side — [Gather](gather.md) and [Spread](spread.md) cover
each in turn.
