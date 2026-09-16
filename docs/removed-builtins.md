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
* [`__import__`](import.md) — loading a module by a name computed at
  runtime is the same escape hatch one level up: `import` already
  names its target statically, and there is no dynamic form behind it
  to call directly
* [`vars`](construction.md#field-reflection-with-fields) — no
  `__dict__` to return; `fields` replaces it
* [`dir`](construction.md#field-reflection-with-fields) — `fields`
  replaces it too, on a module: an ordered, documented walk instead
  of an unordered list of bare names
* [`next`](for-and-while.md#explicit-iteration) — advancing an
  iterator by hand is rare enough that it needs no builtin: `Iterator`
  declares `next` as an ordinary method, `cursor.next()`, not a
  dunder with a wrapper function in front of it
* `ascii` is not a bare builtin. It moves to `string.ascii(...)`, in
  the `string` module — a specific-audience string-formatting
  operation, the same reasoning that keeps `iteration.done` in the
  `iteration` module instead of built in.
* `filter` — a comprehension with an `if` clause already does this:
  `[x for x in y if condition]`, one spelling instead of two
* `globals` is supplanted by `locals` — nothing else. There is no
  separate global scope to reach past locals for: at any point in the
  source, everything visible already is the locals, the same reason
  assignment is always local once [`global` is
  gone](scope.md#no-global) — nothing privileges module scope as a
  separately reachable, dict-shaped thing `globals()` could hand back
  that `locals()` doesn't already cover
* `compile` — not a bare builtin; tucked into a library alongside
  other code-object and reflection tools, not exposed at the top level
* [`delattr`](classes.md#no-del-on-fields) — the same rule `del
  obj.field` already breaks on; a declared field is part of a class's
  fixed shape, not an optional slot a call can remove
* `open` — not a bare builtin; `Path.open(...)` already exists
  alongside it in Python, so keeping both is one spelling too many
* [`bin`, `oct`, `hex`](strings.md#base-formatted-string-factories) —
  not bare builtins; they move to `str.bin`, `str.oct`, and `str.hex`,
  named factories instead of three unrelated top-level names for the
  same job
* `chr`, `ord` — not bare builtins; they move to `string.chr(...)`
  and `string.ord(...)`, the same specific-audience move `ascii`
  already makes
* [`bytes`, `bytearray`, `memoryview`](binary-types.md) — not
  separate conversion functions; `Bytes(x)`, `ByteArray(x)`, and
  `MemoryView(x)` already construct through each type's own default
  factory, the same way any other class is constructed, so there is
  no second, lowercase name left to call instead
