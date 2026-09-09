# Main ideas

Lucid is a Python-like language sketch that keeps Python easy to read and
write, poaches the best ideas other languages already found, and removes
the compatibility constraints that keep Python from adopting many of its
own best proposals.

Core principle:

    Keep Python's directness, make structure explicit, and choose the cleaner
    rule when compatibility no longer has to win.

## Zero-deprecation

Python's deprecations stay supported for several versions before removal,
so old designs often outlive their reasons. Lucid cuts that to a one-year
cadence, each release shipping LLM upgrade instructions instead of a
deprecation period. This freedom is a standing property, not a founding
choice: Lucid treats rejected or constrained ideas as open design space by
default. See [Keyword reference](keywords.md).

## Python readability

Lucid code stays as easy to read and write as ordinary Python: indentation
matters, definitions are direct, and simple programs need no ceremony
(see [Binding](names.md), [If](if.md), [For and While](for-and-while.md)).
Where Python lets more than one way survive — three generations of
string formatting, or a named-fields bag as a class, `dataclass`,
`NamedTuple`, or `TypedDict` — [Zero-deprecation](#zero-deprecation)
lets Lucid pick one and enforce it by construction:
[No tuple or namedtuple type](collections.md) picked named records once.

## Explicit over implicit

Python lets behavior happen somewhere other than where you're looking —
`__getattr__`, descriptors, and metaclasses intercept normal-looking
code, and `typing.Protocol` grants conformance to code that never asked
for it. Lucid closes these off: traits are nominal
([No structural traits](traits.md)); `final` and `override` must be
written, never inferred ([Explicit overrides](traits.md)); attribute
access has no interception hooks
([No descriptors](classes.md#no-descriptors)).

## Java-style single inheritance

Python's multiple inheritance overloads one base-class list for shared
state, interfaces, reusable behavior, and MRO — jobs that interfere,
since MRO order can silently change which method a call reaches. Lucid
keeps Java's split instead, naming the non-state-owning kind `trait`:
one class parent, since only a class owns stored state, but any number
of traits. See [Traits](traits.md) and
[One class parent](class-inheritance.md).

## Scala-style type information

Definition-site type relationships are checked and versioned; inferred
variance can flip unintentionally, and a docstring's mutation promise
isn't checked at all. Generic parameters carry definition-site variance
with `+K`, `-K`, `=K`; mutable, read-only, and immutable views are
visible with `T`, `~T`, `!T`. See [Generics](generics.md),
[Mutability](mutability.md), and
[Modern type specification](type-specification.md).

## Julia-style dynamic dispatch

Python's binary operators are single-dispatch on the left operand, so a
second method and a negotiation protocol — `__radd__`, `NotImplemented`
— exist only to approximate the two-sided decision `a + b` needs. Lucid
makes operators ordinary multiple-dispatch functions instead, picking an
implementation from every argument's type at once, staying open to third
parties the same way [Dispatch beyond operators](dispatch.md) already is
for ordinary functions.

## Rust-style error handling

Python collapses two kinds of failure into one mechanism — `raise`/
`try`/`except` handle both an expected outcome and a broken invariant,
with nothing in a function's signature saying which. Lucid splits the
two: a recoverable failure is an ordinary return type, checked
exhaustively like any other union, with `?` as sugar to propagate it;
`raise` stays, narrowed to broken invariants, unchecked. See
[Results](results.md) and [Exceptions](exceptions.md).

## Basedpython-style loop bindings

Python's `for` loop reuses one binding across every iteration, so a
closure created in the loop body captures that shared variable, not the
value it appeared to capture — `fns = []; for i in [1, 2, 3]:
fns.append(def(): print(i))` prints `3 3 3`. Lucid gives each iteration
a fresh binding instead: the closures above print `1 2 3`. See
[Fresh loop bindings](for-and-while.md#fresh-loop-bindings).

## Kotlin-style function types

Python spells a callable's type `Callable[[A, B], R]`, inherited from
fitting a parameter list inside the same generic syntax as every other
type — it reads nothing like the `def` it describes. Lucid spells it
`(A, B) -> R`, matching Kotlin: the same `->` a `def`'s own return type
already uses, with the same grammar an ordinary signature uses for
names, zoning, or variadic gathering. See [Function types](types.md).
