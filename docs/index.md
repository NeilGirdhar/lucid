# The Lucid language

## Motivation

Lucid is a language for LLMs: a Python-like language sketch designed to be
as easy for an LLM to read and write correctly as for a human. The two
goals overlap far more than they conflict — an LLM benefits from the same
explicitness, predictability, and freedom from hidden state that make code
readable to a person, so most of what follows serves both audiences at
once. But the two audiences are not identical, and where they pull apart,
this spec picks a side deliberately rather than papering over the conflict.

Typing cost is one place they diverge. A human pays, in effort felt at the
keyboard, for every extra character a call site needs; an LLM, or a linter
inserting the same text mechanically, does not. [Anonymous
functions](calls.md#anonymous-functions)'s explicit `def: expr` for a lazy
argument is one consequence: a dedicated auto-thunking parameter would save
a human a few keystrokes, but since the one writing `def:` out is usually
an LLM or a linter, there is no keystroke cost left to design sugar around.

Naming consistency is another. An LLM has no entrenched habit to offend by
capitalizing `str` and `int` the same way [`Bytes` and
`ByteArray`](builtins.md) already are; a human reader does, from decades of
exactly this spelling elsewhere. Lucid keeps `bool`/`int`/`float`/`complex`/`str`
lowercase for that reason alone — a concession to the human reader this
spec would have no cause to make if it optimized for an LLM audience only.

## Guiding principles

Four properties this spec keeps returning to, each one a standard humans
already ask of readable code, held here without exception:

### Succinct

**A simple idea is written simply, with nothing carried along out of
habit.**

* no repeated boilerplate for one job
    * Python's `*args`, `**kwargs`, and a `ParamSpec` to forward them
      typed collapse into one gathered value:
      `***rest: Arguments[str, dict[str, str]]`
* no cost for the common case
    * mutable by default, no `~`; visible by default, only a leading
      `_` costs anything
* no separate machinery for what the grammar already expresses
    * `functools.partial(score, weights)` becomes `score(weights, _)`
      — an ordinary call, with a hole

### Clear

**Nothing about a piece of code's behavior depends on something declared
elsewhere the reader never saw.**

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
    * `assert (x == y, "message")` is always true in Python — Lucid's
      `assert` requires those parens, so there is no bare form left
      to mis-parenthesize

### Checked

**A mistake is caught where it is made, not learned three calls later at
runtime.**

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

### Capable

**None of the above is bought by cutting scope, so "easy to get right"
never has to mean "too small to use."**

* generics carry real variance, not just erased type parameters
    * `trait Cache[in out K, in out V]` — definition-site `in`/`out`
      variance, not a call-site-only or fully erased scheme
* binary operators dispatch on both operand types
    * `a + b` picks an implementation from both operands' types at
      once — no `__radd__`, no `NotImplemented` negotiation
* errors stay structured, not reduced to a bare boolean or `None`
    * a recoverable failure is an ordinary return type, checked
      exhaustively; `raise` stays for what should never happen

## Example

```python
trait Scorable[in K]:
    def score(self, item: K) -> float

    def is_confident(self, item: K) -> bool:
        return self.score(item) >= 0.8

class InferenceModel[in ~out K](Scorable[K]):
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

def evaluate(model: InferenceModel[str], item: str) -> float:
    return model.score(item)

model: InferenceModel[str] = InferenceModel.from_checkpoint("model.bin", ["cat", "dog"])
stable: !InferenceModel[str] = freeze(model)
view: ~InferenceModel[str] = model
view.label_count            # fine: a getter is read-only by construction
view.score("cat")           # error: score mutates, view is read-only
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
