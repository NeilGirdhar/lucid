# Identity and instance checks

Python's dominant runtime check is `isinstance(x, T)`; plain `is` for
object identity is rare outside `is None`. Lucid swaps the two: `is`
and `is not` become instance checks, and identity moves to `===` and
`!==`, borrowed from JavaScript.

| Lucid | Meaning |
| --- | --- |
| `x is T` | `isinstance(x, T)` |
| `x is not T` | `not isinstance(x, T)` |
| `x === y` | identity |
| `x !== y` | not identity |

A right-hand side naming a class, trait, or union alias is a
type — the swap above applies. A right-hand side whose own static type
is an *instance*, not a class, keeps plain identity instead: `none` is
the ordinary case, so `x is none` and `x is not none` read exactly
as they always have. The same holds for any other singleton value.

A test whose tested type shares no value with the subject's own type can
never hold. Lucid rejects it instead of leaving a dead branch, or a
mistyped name, to be found at runtime:

```python
def f(x: none, w: Widget):
    if x is int: ...   # error: none and int share no value
    if w is str: ...   # error: Widget and str share no value
```
The type tested is the subject's own narrowed type, which is often
sharper than its declaration — [a constructor call infers as final A](construction.md), not plain `A`, so the check catches a subclass test
that can never hold too, not only an unrelated type:

```python
class Dog(Animal): ...

def g():
    a = Animal()
    if a is Dog: ...   # error: an exactly-Animal value is never a Dog
```
`==` is unchanged: it still resolves through [Multiple dispatch](dispatch.md), exactly as before. Only `is`/`is not` and
`===`/`!==` change meaning.
