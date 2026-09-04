The Lucid language
==================

Lucid is a Python-like language sketch that keeps Python easy to read and write
while making room for cleaner type, dispatch, object, and compatibility rules.

Core principle:

    Keep Python's directness, make structure explicit, and choose the cleaner
    rule when compatibility no longer has to win.

Lucid borrows Python's readable surface syntax, Scala-style definition-site type
information, Julia-style multiple dispatch, dataclasses' transparent field-first
objects, and the freedom to follow Python Enhancement Proposals that Python
could not adopt because of backward compatibility.

Object state is declared in the class body. Construction returns fully built
objects. Public module APIs are marked with ``export``. Interfaces declare
obligations, traits provide reusable behavior, and binary operators dispatch on
both operands. Generic parameters carry definition-site variance with ``+K``,
``-K``, and ``=K``. Mutable, read-only, and immutable views are visible in the
type spelling with ``T``, ``T?``, and ``T!``.

Example
-------

.. code-block:: python

   export interface Scorable[+K]:
       declare score(self, item: K) -> float

   export trait ScoreBands[+K](Scorable[K]):
       def is_confident(self, item: K) -> bool:
           return self.score(item) >= 0.8

   export class InferenceModel[=K](Scorable[K], ScoreBands[K]):
       weights: Tensor
       labels: list[K]
       scores: dict[K, float]

       factory from_checkpoint(cls, path: Path, labels: list[K]):
           weights = Tensor.load(path)
           return construct(weights, labels, {:})

       def score(self, item: K) -> float:
           if item not in self.scores:
               self.scores[item] = self.weights.dot(encode(item))
           return self.scores[item]

       getter label_count(self) -> int:
           return len(self.labels)

   def evaluate(model: InferenceModel?[str], item: str) -> float:
       return model.score(item)

   model: InferenceModel[str] = InferenceModel.from_checkpoint("model.bin", ["cat", "dog"])
   stable: InferenceModel![str] = freeze(model)

This example shows several core language mechanics in one place:

* exported definitions are explicitly public
* interfaces use ``declare`` for required behavior
* traits provide reusable bodies
* stored fields are declared in the class body
* factories construct exact, fully initialized objects
* getters expose computed attributes without descriptors
* mutable, read-only, and immutable views are visible in annotations

Documentation
-------------

Continue with the specification documents, front-to-back:

.. list-table::
   :header-rows: 1

   * - Document
     - Purpose
   * - `Main ideas <docs/principles.rst>`_
     - The five ideas that shape the rest of the language.
   * - `Source basics and names <docs/source-and-names.rst>`_
     - File layout and indentation, and how names, assignment, and scope
       work.
   * - `Types, mutability, and annotations <docs/types.rst>`_
     - What ``type`` means in Lucid versus Python, visible type contracts,
       type expressions and the ``type`` keyword, recursive type aliases,
       definition-site variance, higher-kinded parameters and existential
       types, mutable/read-only/immutable views,
       and exact numeric types.
   * - `Strings and collections <docs/collections.rst>`_
     - String and collection literals, TypedDict shapes, records instead of
       tuples, and ``skip`` elision.
   * - `Calls, indexing, and operators <docs/operators.rst>`_
     - Call and indexing syntax.
   * - `Multiple dispatch <docs/dispatch.rst>`_
     - Why binary operators dispatch on both operands, dispatch requirements
       in interfaces, ambiguous dispatch, dispatch across projects and
       hierarchies, and dispatch for ordinary functions.
   * - `Control flow and statements <docs/control-flow.rst>`_
     - Conditionals, loops, ``if_broken``, exhaustive pattern matching, and
       unspecified statements.
   * - `Modules, projects, and public APIs <docs/modules.rst>`_
     - Re-exports, lazy imports, and how ``project.yaml``/``development.yaml``
       fit in.
   * - `Modern type specification <docs/type-specification.rst>`_
     - The interface, trait, and class model for user-defined types.
   * - `Project configuration <docs/project-configuration.rst>`_
     - The structure of ``project.yaml`` and ``development.yaml``, and how
       they replace ``pyproject.toml`` and ``__init__.py``.
   * - `Keyword reference <docs/keywords.rst>`_
     - Every keyword Lucid preserves, discards, and adds, in one place.

These eleven documents are the source of truth for Lucid semantics.

Current slogan
--------------

    Classes store data. Interfaces specify obligations. Traits provide reusable behavior. Factories construct exact classes. Attribute access is structural, not magical. Exports define the public API.
