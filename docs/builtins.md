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
* [`NotImplemented`](dispatch.md#no-notimplemented-operator-negotiation)
  — multiple dispatch replaces the reflected-method negotiation
  protocol it signals
* [`isinstance`](identity-checks.md), [`issubclass`](identity-checks.md)
  — `is`/`is not` become instance checks; identity moves to `===`/`!==`
