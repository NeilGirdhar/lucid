# Anonymous class

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
grammar further; see [Parameters](parameters.md).

[Class members](class-members.md) covers the named kind's own
vocabulary — fields, methods, getters, setters, and the rest.
