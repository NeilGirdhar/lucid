# The Lucid language

Lucid is a Python-like language sketch that keeps Python easy to read and write
while making room for cleaner type, dispatch, object, and compatibility rules.

Core principle:

    Keep Python's directness, make structure explicit, and choose the cleaner
    rule when compatibility no longer has to win.

Lucid borrows Python's readable surface syntax, Java-style single class
inheritance plus multiple traits, Scala-style definition-site type
information, Julia-style multiple dispatch, basedpython's fresh
per-iteration loop bindings, Kotlin-style function types, and Rust's
split between recoverable and unrecoverable errors — made possible by a
zero-deprecation release cadence that lets Lucid choose the cleaner rule
instead of the Python-compatible one throughout the language.

Object state is declared in the class body. Construction returns fully built
objects. A definition is visible everywhere in the project by default; a
leading `_` makes it private instead. Traits declare obligations,
provide reusable behavior, or both at once, and binary operators dispatch
on both operands. Generic parameters carry definition-site variance with
`+K`, `-K`, and `=K`. Mutable, read-only, and immutable views are
visible in the type spelling with `T`, `~T`, and `!T`.

## Example

```python
trait Scorable[+K]:
    def score(self, item: K) -> float

    def is_confident(self, item: K) -> bool:
        return self.score(item) >= 0.8

class InferenceModel[+=K](Scorable[K]):
    weights: Tensor
    labels: list[K]
    _scores: dict[K, float]

    factory from_checkpoint(cls, path: Path, labels: list[K]):
        weights = Tensor.load(path)
        return construct(weights, labels, {:})

    def score(self, item: K) -> float:
        if item not in self._scores:
            self._scores[item] = self.weights.dot(encode(item))
        return self._scores[item]

    getter label_count(self) -> int:
        return len(self.labels)

def evaluate(model: ~InferenceModel[str], item: str) -> float:
    return model.score(item)

model: InferenceModel[str] = InferenceModel.from_checkpoint("model.bin", ["cat", "dog"])
stable: !InferenceModel[str] = freeze(model)
```
This example shows several core language mechanics in one place:

* `_scores` is private to the class; everything else here is visible
  project-wide with no keyword needed
* a trait member with no body is a requirement, one with a body is
  reusable behavior — no marker keyword needed for either
* stored fields are declared in the class body
* factories construct exact, fully initialized objects
* getters expose computed attributes without descriptors, attribute
  access stays structural rather than programmable through hooks
* mutable, read-only, and immutable views are visible in annotations

## Documentation

Continue with the specification documents. Nesting groups related documents
under one theme; within a theme, and across the list top to bottom, each
document builds mostly on documents already covered above it:

* [Main ideas](principles.md) — the nine ideas behind the language.
* Names

  * [Binding](names.md) — ordinary binding, final locals, and the
    black-hole target `_`.
  * [Destructuring](destructuring.md) — destructuring with `let` and
    with `match`.
  * [Scope](scope.md) — scope-rebinding rules.

* Types, mutability, and annotations

  * [Type vocabulary](types.md) — what a type is, and type-level
    expressions.
  * [Match types](match-types.md) — computing a type from a type's
    own structure.
  * [Casting](casting.md) — `trust`, in place of a general-purpose
    `cast`.
  * [Mutability](mutability.md) — mutable, read-only, and
    immutable views.
  * [Generics](generics.md) — variance, higher-kinded parameters,
    and existentials.
  * [Numeric types](numeric-types.md) — exact numeric types and
    capability traits.

* Traits and classes

  * [Overview](type-specification.md) — why traits and classes
    are separate.
  * [Traits](traits.md) — obligations and reusable behavior, in
    place of multiple inheritance.
  * [Classes](classes.md) — object shape, members, and
    inheritance.
  * [Construction](construction.md) — factories, field
    reflection, and call-site captured values.

* [Multiple dispatch](dispatch.md) — dispatch on both operands,
  and beyond operators.
* Control flow

  * [Control flow and statements](control-flow.md) — conditionals,
    loops, matching, and errors.
  * [Context managers](context-managers.md) — the
    `contextmanager` modifier.

* [Strings and collections](collections.md) — literals, records,
  and TypedDict shapes.
* [Indexing](indexing.md) — indexing and unpacking.
* [Calls](calls.md) — call syntax, partial application, and
  anonymous functions.
* Parameters and decorators

  * [Parameters and arguments](parameters.md) — the anonymous
    class, and argument gathering.
  * [Decorators](decorators.md) — identity-preserving `@`, and
    decorator factories.

* [Project configuration](project-configuration.md) —
  `project.yaml`, `development.yaml`, and `lucid.lock`.
* [Modules, projects, and public APIs](modules.md) —
  module-private names, and lazy imports.
* [Keyword reference](keywords.md) — every keyword, in one place.
* [Rejected features](rejected-features.md) — other languages'
  features that turned out to already be covered.

These documents are the source of truth for Lucid semantics.

