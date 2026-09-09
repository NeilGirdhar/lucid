# Builtins

* Lowercase constants: `false`, `none`, `true`
* Basic types, listed in the order each is first defined:
  [`bool`](numeric-types.md#exact-bool), [`int`](numeric-types.md#exact-int),
  [`float`](numeric-types.md#exact-float-and-float-like-input),
  [`complex`](numeric-types.md#exact-complex), and [`str`](strings.md)
* Capability traits, the main ABC-equivalent nominal traits (see
  [No structural traits](traits.md#no-structural-traits)), listed in
  the order each is first defined:
  [`Eq`](mutability.md#equality-ordering-and-hashing),
  [`Ord`](mutability.md#equality-ordering-and-hashing),
  [`Hashable`](mutability.md#equality-ordering-and-hashing),
  [`Sized`](numeric-types.md#exact-bool),
  [`Iterable`](for-and-while.md#explicit-iteration),
  [`Iterator`](for-and-while.md#explicit-iteration),
  [`Container`](strings.md#strings-are-not-sequences),
  [`Collection`](strings.md#strings-are-not-sequences), and
  [`Sequence`](strings.md#strings-are-not-sequences)
* Special types, built outside the ordinary class system, listed in
  the order each is first defined: [`object`](types.md#no-any-escape-hatch),
  [`Arguments`](arguments.md), [`Parameters`](parameters.md),
  [`SourceLocation`](call-site-captured-values.md#caller-captured-source-locations),
  [`VarName`](call-site-captured-values.md#name-captured-identifiers),
  [`Sentinel`](call-site-captured-values.md#name-captured-identifiers),
  [`Self`](class-members.md#read-only-methods-with-self), and
  [`DottedPath`](project-yaml.md#no-__module__-or-__qualname__)

## Builtin functions

* [`abs`](numeric-types.md#capability-traits),
  [`round`](numeric-types.md#capability-traits),
  [`len`](strings.md#strings-are-not-sequences) — kept, each
  dispatched through the capability trait that already covers it
  (`SupportsAbs[+K]`, `SupportsRound[+K]`, `Sized`) rather than a
  single hardcoded signature — `abs` returns `int` for an `int` and
  `float` for a `complex`, the type-level consequence of dispatching
  on the argument's own type instead of calling one fixed method.
* [`fields`](construction.md#field-reflection-with-fields) — walks a
  class's own fields, dispatched on whether it's given an instance or
  the class itself, replacing Python's `vars()`: Lucid has no
  `__dict__` for `vars()` to return, and `fields` gives back something
  `vars()` never could — declared order, each field's docstring, and
  its metadata, not just a name/value pair.
* `zip` — always strict. Mismatched-length iterables are a runtime
  error, never silent truncation to the shortest — the same class of
  mistake `assert`'s required parentheses closes elsewhere, just for
  iteration instead of assertion. Python's own `zip` needs an explicit
  `strict=True` to opt into this; Lucid has no other mode.
* `any`, `all` — kept as ordinary functions. `any` the function and
  `any` the keyword ([Existential types](generics.md#existential-types))
  are distinguished the same way `type`/`match`/`not` already are
  elsewhere: by position. `any(xs)` is a call; `any Trait` is bare, no
  parentheses, and never appears where a call would.
* `iter`, `locals` — kept, unchanged.
* [`next`](for-and-while.md#explicit-iteration) — kept, but returns
  `IterResult[T]` instead of raising `StopIteration`, wrapping
  `__next__` the same way `len` already wraps `__len__` — nothing
  consuming an iterator by hand has to touch the dunder directly.
* [`super`](traits.md#explicit-overrides) — kept, but takes no
  parentheses: `super.save()`, not `super().save()`. A class has at
  most one class parent, so calling through to the overridden
  implementation is never ambiguous the way it can be in Python's own
  cooperative multiple inheritance — there's nothing to call, only the
  one parent to name.

[Removed builtins](removed-builtins.md) covers the rest of Python's
`builtins` module: the names a specific design decision elsewhere in
this spec actually removes or replaces.
