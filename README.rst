The Lucid language
==================

Lucid is a Python-like language sketch that keeps Python easy to read and write
while making room for cleaner type, dispatch, object, and compatibility rules.

Core principle:

    Keep Python's directness, make structure explicit, and choose the cleaner
    rule when compatibility no longer has to win.

Lucid borrows Python's readable surface syntax, Java-style single class
inheritance plus multiple interfaces, Scala-style definition-site type
information, Julia-style multiple dispatch, basedpython's fresh
per-iteration loop bindings, Kotlin-style function types, and Rust's
split between recoverable and unrecoverable errors — made possible by a
zero-deprecation release cadence that lets Lucid choose the cleaner rule
instead of the Python-compatible one throughout the language.

Object state is declared in the class body. Construction returns fully built
objects. A definition is visible everywhere in the project by default; a
leading ``_`` makes it private instead. Interfaces declare obligations,
traits provide reusable behavior, and binary operators dispatch on both
operands. Generic parameters carry definition-site variance with ``+K``,
``-K``, and ``=K``. Mutable, read-only, and immutable views are visible in the
type spelling with ``T``, ``&T``, and ``!T``.

Example
-------

.. code-block:: python

   interface Scorable[+K]:
       def score(self, item: K) -> float

   trait ScoreBands[+K](Scorable[K]):
       def is_confident(self, item: K) -> bool:
           return self.score(item) >= 0.8

   class InferenceModel[=K](Scorable[K], ScoreBands[K]):
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

   def evaluate(model: &InferenceModel[str], item: str) -> float:
       return model.score(item)

   model: InferenceModel[str] = InferenceModel.from_checkpoint("model.bin", ["cat", "dog"])
   stable: !InferenceModel[str] = freeze(model)

This example shows several core language mechanics in one place:

* ``_scores`` is private to the class; everything else here is visible
  project-wide with no keyword needed
* interfaces require behavior with a bodyless member, no marker keyword needed
* traits provide reusable bodies
* stored fields are declared in the class body
* factories construct exact, fully initialized objects
* getters expose computed attributes without descriptors, attribute
  access stays structural rather than programmable through hooks
* mutable, read-only, and immutable views are visible in annotations

Documentation
-------------

Continue with the specification documents. Nesting groups related documents
under one theme; within a theme, and across the list top to bottom, each
document builds mostly on documents already covered above it:

* `Main ideas <docs/principles.rst>`_ — the nine ideas behind the language.
* `Names, binding, and scope <docs/names.rst>`_ — binding, destructuring,
  and scope.
* Types, mutability, and annotations

  * `Type vocabulary <docs/types.rst>`_ — what a type is, and type-level
    expressions.
  * `Mutability <docs/mutability.rst>`_ — mutable, read-only, and
    immutable views.
  * `Generics <docs/generics.rst>`_ — variance, higher-kinded parameters,
    and existentials.
  * `Numeric types <docs/numeric-types.rst>`_ — exact numeric types and
    capability interfaces.

* Modern type specification

  * `Overview <docs/type-specification.rst>`_ — why interfaces, traits,
    and classes are separate.
  * `Interfaces <docs/interfaces.rst>`_ — obligations, without state or
    bodies.
  * `Traits <docs/traits.rst>`_ — reusable behavior, in place of multiple
    inheritance.
  * `Classes <docs/classes.rst>`_ — object shape, members, and
    inheritance.
  * `Construction <docs/construction.rst>`_ — factories, field
    reflection, and call-site captured values.

* `Multiple dispatch <docs/dispatch.rst>`_ — dispatch on both operands,
  and beyond operators.
* Control flow

  * `Control flow and statements <docs/control-flow.rst>`_ — conditionals,
    loops, matching, and errors.
  * `Context managers <docs/context-managers.rst>`_ — the
    ``contextmanager`` modifier.

* `Strings and collections <docs/collections.rst>`_ — literals, records,
  and TypedDict shapes.
* `Indexing <docs/indexing.rst>`_ — indexing and unpacking.
* `Calls <docs/calls.rst>`_ — call syntax, partial application, and
  anonymous functions.
* Parameters and decorators

  * `Parameters and arguments <docs/parameters.rst>`_ — the anonymous
    class, and argument gathering.
  * `Lazy parameters <docs/lazy-parameters.rst>`_ — deferring an
    argument's evaluation to whether it's needed.
  * `Decorators <docs/decorators.rst>`_ — identity-preserving ``@``, and
    decorator factories.

* `Project configuration <docs/project-configuration.rst>`_ —
  ``project.yaml``, ``development.yaml``, and ``lucid.lock``.
* `Modules, projects, and public APIs <docs/modules.rst>`_ —
  module-private names, and lazy imports.
* `Keyword reference <docs/keywords.rst>`_ — every keyword, in one place.

These documents are the source of truth for Lucid semantics.
