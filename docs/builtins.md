# Builtins

* Lowercase constants: `false`, `none`, `true`
* Basic types, listed in the order each is first defined:
  [`bool`](numeric-types.md#exact-bool), [`int`](numeric-types.md#exact-int),
  [`float`](numeric-types.md#exact-float-and-float-like-input),
  [`complex`](numeric-types.md#exact-complex), [`str`](strings.md)
  (see also its
  [`bin`/`oct`/`hex` factories](strings.md#base-formatted-string-factories)),
  and [`Bytes`, `ByteArray`, and `MemoryView`](binary-types.md) — the
  last three capitalized, unlike the others here, since
  `bytes`/`bytearray`/`memoryview` stay the ordinary lowercase
  conversion calls, not the type names. `bool`/`int`/`float`/`complex`/`str`
  have no such split to force a second name, so they keep the one
  spelling doing both jobs — the deeper consistency an all-caps
  `Int`/`Str` would buy costs a human reader decades of `int`/`str`
  habit to unlearn, for a distinction an LLM reader has no such habit
  to notice in the first place
* Capability traits, the main ABC-equivalent nominal traits (see
  [No structural traits](traits.md#no-structural-traits)), listed in
  the order each is first defined:
  [`Callable`](types.md#the-callable-trait),
  [`Eq`](mutability.md#equality-ordering-and-hashing),
  [`Ord`](mutability.md#equality-ordering-and-hashing),
  [`Hashable`](mutability.md#equality-ordering-and-hashing),
  [`Sized`](numeric-types.md#exact-bool),
  [`Iterable`](for-and-while.md#explicit-iteration),
  [`Iterator`](for-and-while.md#explicit-iteration),
  [`Reversible`](for-and-while.md#reversible-and-reversed),
  [`Set`](collections.md#the-set-trait),
  [`Container`](strings.md#strings-are-not-sequences),
  [`Collection`](strings.md#strings-are-not-sequences),
  [`Sequence`](strings.md#strings-are-not-sequences), and
  [`Buffer`](binary-types.md)
* Special types, built outside the ordinary class system, listed in
  the order each is first defined: [`object`](types.md#no-any-escape-hatch),
  [`Arguments`](arguments.md), [`Parameters`](parameters.md),
  [`SourceLocation`](call-site-captured-values.md#caller-captured-source-locations),
  [`VarName`](call-site-captured-values.md#name-captured-identifiers),
  [`Sentinel`](call-site-captured-values.md#name-captured-identifiers),
  [`Self`](class-members.md#read-only-methods-with-self),
  [`DottedPath`](project-yaml.md#no-__module__-or-__qualname__), and
  [`Module`](construction.md#field-reflection-with-fields)

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
  class's, trait's, or module's own members, dispatched on which it's
  given (an instance or the class itself included), replacing both
  Python's `vars()` (no `__dict__` to return) and `dir()` (an ordered,
  documented walk instead of an unordered list of bare names).
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
* `iter`, `locals`, `format`, `hash`, `help`, `sum`, `reversed`,
  `repr`, `print`, `list`, `set`, `dict`, `enumerate`, `map`, `max`,
  `min`, `sorted`, `slice`, `getattr`, `setattr`, `hasattr` — kept,
  unchanged.
* [`pow`](numeric-types.md#pow-dispatches-per-type) — kept, but
  multiple-dispatch, one case per base type, so the zero-base,
  negative-exponent case returns each type's own `inf` instead of
  raising, the same trade floor division and modulo already make.
* [`super`](traits.md#explicit-overrides) — kept, but takes no
  parentheses: `super.save()`, not `super().save()`. A class has at
  most one class parent, so calling through to the overridden
  implementation is never ambiguous the way it can be in Python's own
  cooperative multiple inheritance — there's nothing to call, only the
  one parent to name.

[Removed builtins](removed-builtins.md) covers the rest of Python's
`builtins` module: the names a specific design decision elsewhere in
this spec actually removes or replaces.
