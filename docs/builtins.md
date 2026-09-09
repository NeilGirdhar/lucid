# Builtins

## Lowercase constants

Lucid spells its boolean and null constants in lowercase:

```text
false none true
```
## Basic types

The exact numeric and string types, listed in the order each is first
defined:

* [`bool`](numeric-types.md#exact-bool)
* [`int`](numeric-types.md#exact-int)
* [`float`](numeric-types.md#exact-float-and-float-like-input)
* [`complex`](numeric-types.md#exact-complex)
* [`str`](strings.md)

## Capability traits

The main ABC-equivalent capability traits, each nominal — a type
satisfies one only by naming it, never merely by shape (see
[No structural traits](traits.md#no-structural-traits)). Listed in the
order each is first defined:

* [`Eq`](mutability.md#equality-ordering-and-hashing)
* [`Ord`](mutability.md#equality-ordering-and-hashing)
* [`Hashable`](mutability.md#equality-ordering-and-hashing)
* [`Sized`](numeric-types.md#exact-bool)
* [`Iterable`](for-and-while.md#explicit-iteration)
* [`Iterator`](for-and-while.md#explicit-iteration)
* [`Container`](strings.md#strings-are-not-sequences)
* [`Collection`](strings.md#strings-are-not-sequences)
* [`Sequence`](strings.md#strings-are-not-sequences)

## Special types

A small number of other built-in types sit outside the ordinary class
system, each recognized by name rather than declared like an ordinary
user-defined type. Listed in the order each is first defined:

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
