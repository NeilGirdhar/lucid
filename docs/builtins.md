# Builtins

## Lowercase constants

Lucid spells its boolean and null constants in lowercase:

```text
false none true
```
## Builtin types

Lucid also ships a small number of built-in types outside the ordinary
class system, each recognized by name rather than declared like an
ordinary user-defined type. Listed in the order each is first defined:

* [`object`](types.md#no-any-escape-hatch) — the root type; assignable
  from anything, but only its own promises are callable on one until
  it is narrowed
* [`Arguments`](arguments.md) — a typed bundle of leftover positional
  and keyword arguments
* [`Parameters`](parameters.md) — `Arguments` plus a fixed, possibly
  zoned prefix
* [`SourceLocation`](call-site-captured-values.md#caller-captured-source-locations)
  — a call's own source location, filled in at the call site
* [`VarName`](call-site-captured-values.md#name-captured-identifiers)
  — a call's own assignment target
* [`Sentinel`](call-site-captured-values.md#name-captured-identifiers)
  — a well-identified singleton, named after the variable it was
  assigned to
* [`Self`](class-members.md#read-only-methods-with-self) — the
  enclosing class, for annotating `self` and return types
* [`DottedPath`](project-yaml.md#no-__module__-or-__qualname__) — a
  symbol's own location, folding `__module__` and `__qualname__` into
  one sequence
