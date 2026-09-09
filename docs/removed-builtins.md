# Removed builtins

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
* [`eval`](types.md#no-any-escape-hatch), [`exec`](types.md#no-any-escape-hatch)
  — running code the checker never saw is exactly the escape hatch
  the type system has none of
* [`vars`](construction.md#field-reflection-with-fields) — no
  `__dict__` to return; `fields` replaces it
* `ascii` is not a bare builtin. It moves to `string.ascii(...)`, in
  the `string` module — a specific-audience string-formatting
  operation, the same reasoning that keeps `iteration.done` in the
  `iteration` module instead of built in.
* `filter` — a comprehension with an `if` clause already does this:
  `[x for x in y if condition]`, one spelling instead of two
* `globals` — there is no such concept. A module has no single mutable
  namespace object to hand back; [Module-private names](modules.md)
  already decides what's visible by the name alone
* `compile` — not a bare builtin; tucked into a library alongside
  other code-object and reflection tools, not exposed at the top level
* [`delattr`](classes.md#no-del-on-fields) — the same rule `del
  obj.field` already breaks on; a declared field is part of a class's
  fixed shape, not an optional slot a call can remove
