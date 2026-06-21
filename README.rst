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

Continue with:

* `Language overview <docs/index.rst>`_
* `The Lucid language <docs/language.rst>`_

Current slogan
--------------

    Classes store data. Interfaces specify obligations. Traits provide reusable behavior. Factories construct exact classes. Attribute access is structural, not magical. Exports define the public API.
