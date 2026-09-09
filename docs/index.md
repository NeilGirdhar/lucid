# The Lucid language

Lucid is a language for LLMs: a Python-like language sketch designed to be as
easy for an LLM to read and write correctly as for a human — which turns out
to mean holding to the same properties good human-readable code already
wants, just refusing to let any of them slide.

Core principle:

    Keep Python's directness, make structure explicit, and choose the cleaner
    rule when compatibility no longer has to win.

Four properties follow from that:

* **Succinct.** A simple idea is written simply, with nothing carried along
  out of habit.
* **Clear.** Nothing about a piece of code's behavior depends on something
  declared elsewhere the reader never saw:

    * no action at a distance
    * no operator that quietly means two different things
    * no well-known footgun a reader has to already know to dodge
    * no second way to spell something already spelled one way

* **Checked.** A mistake is caught where it is made, not learned three calls
  later at runtime:

    * exhaustive matches
    * no silent escape hatch out of the type system
    * class shapes closed by default
    * private names enforced rather than merely requested

* **Capable.** None of the above is bought by cutting scope. Generics,
  multiple dispatch, and a real error-handling story are all still here, so
  "easy to get right" never has to mean "too small to use."

Lucid borrows Python's readable surface syntax, Java-style single class
inheritance plus multiple traits, Scala-style definition-site type
information, Julia-style multiple dispatch, Kotlin-style function types,
and Rust's split between recoverable and unrecoverable errors — made
possible by a zero-deprecation release cadence that lets Lucid choose the
cleaner rule instead of the Python-compatible one throughout the language.

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
