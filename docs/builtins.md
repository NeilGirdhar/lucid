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
* `ascii` is not a bare builtin. It moves to `string.ascii(...)`, in
  the `string` module — a specific-audience string-formatting
  operation, the same reasoning that keeps `iteration.done` in the
  `iteration` module instead of built in.

## Python builtins not in Lucid

Python's `builtins` module has far more names than this specification
has had reason to address — most of it, unaddressed, simply carries
over unchanged. This lists only the ones a specific design decision
elsewhere in this spec actually removes or replaces, each linked to
that decision:

* [`tuple`](collections.md#no-tuple-or-namedtuple-type) — no tuple
  type; use a class or `!list`
* [`property`](classes.md#no-property) — `getter`/`setter` member
  syntax instead
* [`staticmethod`](class-members.md#no-static-methods-for-namespaced-functions)
  — a function that needs no `self` or `cls` stays a function
* [`classmethod`](keywords.md#new-lucid-keywords) — a modifier
  keyword, not a decorator; a class method is declared, not wrapped
* [`NotImplemented`](dispatch.md#no-notimplemented-operator-negotiation)
  — multiple dispatch replaces the reflected-method negotiation
  protocol it signals
* [`isinstance`](identity-checks.md), [`issubclass`](identity-checks.md)
  — `is`/`is not` become instance checks; identity moves to `===`/`!==`
* [`frozenset`](collections.md#immutable-collection-literals) — as a
  call, replaced by the `!` marker on a set literal (`!{a, b}`); the
  type itself still exists, just not this constructor
* [`type`](types.md#what-class-means-in-lucid) — as a call returning a
  value's own class, since `type` is a keyword now; `class[X]` covers
  the annotation `type[X]` used to
* [`next`](for-and-while.md#explicit-iteration) — an iterator's
  `__next__` returns `IterResult[T]` directly instead of raising
  `StopIteration`, so `next()`'s whole contract (catch the exception,
  or don't) has nothing left to do; call `__next__()` and match the
  result
* [`eval`](types.md#no-any-escape-hatch), [`exec`](types.md#no-any-escape-hatch)
  — running code the checker never saw is exactly the escape hatch
  the type system has none of
* [`vars`](construction.md#field-reflection-with-fields) — no
  `__dict__` to return; `fields` replaces it
* `ascii` — not a bare builtin; see [Builtin functions](#builtin-functions)
