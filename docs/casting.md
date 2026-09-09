# Casting

A Python value crossing into Lucid with no further information is typed as
`object` — nothing is assumed about it, the same as any other value whose
shape is genuinely unknown. That is not a new rule; it is the
[no-`Any` principle](types.md#no-any-escape-hatch) applied to
values that happen to come from outside Lucid, instead of values that
happen to be under-specified inside it.

Getting anything more specific out of an `object` that actually came from
Python requires an explicit, unverified claim: `trust`.

```python
raw: object = some_python_function()
items: !list[int] = trust[!list[int]](raw)
```
`trust` asserts a type with no proof behind it — there is nothing on the
Python side for the checker to verify against — but the claim is visible,
written once, at the exact place it is made. That is the difference from
`Any`: `Any` lets a value be used as anything, anywhere, with no marker
recording that a leap was taken; `trust` requires writing down exactly
what is being trusted, and where, every time.

`trust` only accepts an `object`-typed operand. Python's `typing.cast`
has no such restriction — it can assert any type in place of any other,
anywhere, purely between values that are already fully typed on the Python
side, with nothing foreign involved at all. Lucid has no equivalent
general-purpose `cast`, on purpose: traits are nominal specifically so
that satisfying one is an explicit, checked act rather than an accidental
shape match, and an unrestricted cast would let any code route around that
check between two ordinary, already-sound Lucid values — `trust[Dog](some_cat)`
between two well-typed Lucid values is not filling a real gap, it is
punching a hole where the checker already had real information. Restricting
`trust` to `object` operands makes that impossible by construction: it
only ever gets to speak where the checker had nothing to say in the first
place, which is exactly and only the Python interop boundary.

Calling `trust` at every use site does not scale to a whole library.
Attaching a claim once, at the import, the way a `.pyi` stub does for
Python's own type checkers, is the natural next step — where such a stub
would live, and how it interacts with lazy imports, is not yet decided.
