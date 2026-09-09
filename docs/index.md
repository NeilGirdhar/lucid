# The Lucid language

Lucid is a language for LLMs: a Python-like language sketch designed to be as
easy for an LLM to read and write correctly as for a human — which turns out
to mean holding to the same properties good human-readable code already
wants, just refusing to let any of them slide.

Core properties:

* **Succinct.** A simple idea is written simply, with nothing carried along
  out of habit:

    * no repeated boilerplate for one job
        * Python's `*args`, `**kwargs`, and a `ParamSpec` to forward them
          typed collapse into one gathered value:
          `***rest: Arguments[str, {str: str}]`
    * no cost for the common case
        * mutable by default, no `~`; visible by default, only a leading
          `_` costs anything
    * no separate machinery for what the grammar already expresses
        * `functools.partial(score, weights)` becomes `score(weights, _)`
          — an ordinary call, with a hole

* **Clear.** Nothing about a piece of code's behavior depends on something
  declared elsewhere the reader never saw:

    * no second way to spell the same thing
        * `tuple`, `namedtuple`, `dataclass`, and a plain class all do the
          same job in Python; Lucid keeps one — a dataclass, spelled with
          just `class`
    * no action at a distance
        * no `__getattr__`, no descriptors, no metaclasses — attribute
          access can't be intercepted by code declared somewhere else
          entirely
    * no operator that means two different things
        * `&` means only intersection; `~` marks a read-only view,
          never `&`
    * no well-known footgun to dodge
        * `assert (x == y, "message")` is always true in Python — a
          non-empty tuple is truthy, and Lucid has no tuple type left to
          build one with

* **Checked.** A mistake is caught where it is made, not learned three calls
  later at runtime:

    * no silent escape hatch out of the type system
        * there is no `Any`; `object` accepts anything but permits only
          what `object` itself promises, until it's narrowed back down
    * exhaustive matches
        * leaving a case out of a `match` over a closed union is a
          compile-time error, not a silent no-op
    * class shapes closed by default
        * `p.z = 3.0` on a class with no declared `z` field is a
          compile-time error, not a new attribute
    * private names enforced rather than merely requested
        * `cache._entries` from outside `Cache` is a compile-time error —
          Python's leading underscore is only a convention nothing checks

* **Capable.** None of the above is bought by cutting scope, so
  "easy to get right" never has to mean "too small to use":

    * generics carry real variance, not just erased type parameters
        * `trait Cache[=K, =V]` — definition-site `+`/`-`/`=` variance,
          not a call-site-only or fully erased scheme
    * binary operators dispatch on both operand types
        * `a + b` picks an implementation from both operands' types at
          once — no `__radd__`, no `NotImplemented` negotiation
    * errors stay structured, not reduced to a bare boolean or `None`
        * a recoverable failure is an ordinary return type, checked
          exhaustively; `raise` stays for what should never happen

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
