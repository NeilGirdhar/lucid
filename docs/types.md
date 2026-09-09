# Type vocabulary

## What `type` means in Lucid

Python overloads the word "type." The builtin `type` returns a value's
class; `type[X]` in an annotation means specifically a class object; and
"type" used loosely in "type annotation" or "type checker" means something
broader than either — any type expression, class or not. Static type
checkers eventually needed a name for that broader sense on its own:
`TypeForm` — a type expression, evaluated to a value, whether or not it
happens to be a single class.

Lucid keeps that distinction, but gives each sense its own name instead of
overloading one word for both. A Lucid `class` is the narrow sense — the
direct analogue of a Python class: nominal, data-owning, producing instances
through construction. A Lucid *type* is the broad sense — the analogue of
`TypeForm`, not of `class` or `type[X]`. Most Lucid types are not
classes at all: `int | none`, `!list[int]`, `dict[str, int]`, an
anonymous record `(x: int, y: int)`, and a TypedDict shape `{...}` are
all types with nothing that could be called a class behind them — type
expressions evaluated to values, the same way `type <type expression>`
(see below) turns any of them into a first-class *type form* to pass around
like any other value.

Every `class` is a type. Not every type is a `class`.

The word itself appears in exactly these roles in Lucid, and no others:

- **The category** — "a type," used the way this section has been using it:
  any type expression evaluated to a value, class or not. This is prose
  vocabulary, not syntax.
- **Type expression**, as a grammar — the parsing mode that applies in
  annotation positions, generic parameter lists, and trait member
  signatures (see [Type expressions and the type keyword](#type-expressions-and-the-type-keyword)), as opposed to
  the ordinary expression grammar used everywhere else. Also not syntax by
  itself — it names which grammar is in effect at a given position.
- `type Name = <type expression>` — the alias statement, giving a type
  expression a name usable in future type positions.
- `type <type expression>` — the prefix operator, reifying a type
  expression into an ordinary value — a type form — usable in ordinary
  expression positions.

Only the last two are actual keyword uses; both are the same keyword,
disambiguated by position rather than by two different words the way
Python's `type`/`type[X]`/`TypeForm` are.

## Visible type contracts

Lucid's annotation syntax is Python's, unchanged: a colon after a name
annotates a field, a parameter, or a local variable, and `->` annotates a
function's return value. Trait obligations are annotated the same way
(see [Modern type specification](type-specification.md)).

```python
class User:
    name: str
    tags: set[str] = {"draft"}

def greet(user: User, times: int = 1) -> str:
    lines: list[str] = [f"Hello, {user.name}"] * times
    return "\n".join(lines)
```
## Function types

A function's type is written the same way its signature already looks:
`(A, B) -> R`, the same spelling Kotlin uses, in place of Python's
`Callable[[A, B], R]`. `->` already means "returns" everywhere else
it appears, so a callable's own type reuses it instead of a second,
bracket-based notation for the same idea:

```python
def process(handler: (int, str) -> bool) -> none:
    ...

add: (int, int) -> int = def(x, y): x + y
```
The arrow nests to the right for a function returning a function:
`(int) -> (str) -> bool` means `(int) -> ((str) -> bool)`. Zero
parameters still needs the parens, `() -> R`; one needs no trailing
comma, since a type position has no bare parenthesized expression for
`(int) -> bool` to be confused with. A named, positional-only, or
keyword-only parameter list uses the same grammar a real signature does
— see [Anonymous class](parameters.md) for the full shape.

This form is recognized only in type positions, the same restriction
already placed on other type-only syntax: `(int) -> int` written where
a value is expected is an ordinary parenthesized expression, not a
callable literal. Lucid also has no gradual, any-arity form — Python's
`Callable[..., R]` — since that is exactly the `Any`-shaped escape
hatch this document already closes off for everyday code; a genuinely
unknown foreign signature stays `object`, claimed with `trust` like
any other untyped value crossing the interop boundary.

## Union types

`A | B` is the type of values that are an `A` or a `B`, the same
spelling Python's own `X | Y` union syntax already settled on —
already used throughout this document, `int | none` among the first
examples above.

## Intersection types

`A & B` is the type of values satisfying both `A` and `B` — the
dual of the union `A | B` just covered.
Lucid's traits are nominal ([Main ideas](principles.md)): a
promise is made only where a class header names it, so a value
assembled from two independently declared traits has nowhere to go
without either a purpose-declared class combining both up front, or a
type that can say "both, whichever value actually provides them":

```python
def render(shape: Drawable & Serializable) -> bytes:
    ...

handlers: list[HasName & HasId]
```
`&` means only this — the read-only mutability view prefix used
everywhere else in this document (`~Self`, `~dict[K, +V]`) was
moved to `~` for exactly this reason, so `&` never has to carry two
meanings at once.

Neither `&` nor `|` has a keyword alternate. `and` and `or`
stay exactly what they already are — value-level boolean operators —
and are never meaningful in a type position; there is one spelling for
each combinator, not two.

## Negation types

`not T` in a type position is the type that excludes `T` — every
value that is not one:

```python
def f(x: not int) -> none:
    ...

f("a")   # ok
f(1)     # error
```
`not` already exists, as ordinary boolean negation; a type position
reads it as type negation instead, the same way this document's own
type-expression grammar already gives several other tokens a second
meaning by position rather than by a second word — `type`, disambiguated
the same way ([What type means in Lucid](#what-type-means-in-lucid)). `not int | str` and
`not (int | str)` differ exactly the way they read: the first excludes
only `int`, leaving `str` untouched by the negation, the second
excludes the whole union. Nesting follows the same rule as every other
type combinator here: parenthesize to bind negation across `|` or
`&`; unparenthesized, it binds to the type immediately next to it.

## Type expressions and the `type` keyword

Wherever a type is expected — variable, parameter, and return annotations,
generic parameter lists, trait member signatures — Lucid parses a *type
expression* rather than an ordinary expression. Most syntax means the same
thing in both grammars (`dict[str, int]`, `~T`, `!T`, and
`Producer[+K]` all evaluate identically either way), but a type expression
can use forms that mean something else, or nothing at all, as an ordinary
expression — for example the TypedDict shape literal in
[Strings and collections](collections.md).

The `type` keyword crosses between the two grammars, in the two directions
that matter:

`type Name = <type expression>`
    A type alias statement. The right-hand side is parsed as a type
    expression, and `Name` becomes usable in future type positions exactly
    as if the aliased expression had been written inline there.

`type <type expression>`
    A prefix operator usable inside an ordinary expression. It parses its
    operand as a type expression and evaluates it to a first-class *type
    form*: an ordinary value, usable wherever ordinary values are, for example
    when passing a type to a metaprogramming function.

```python
type Config = !InferenceModel[str]
value: Config = freeze(model)

form = type list[str]                                # an ordinary value: a reified type
handlers = {"json": JSONHandler, "xml": XMLHandler}   # an ordinary dict, not a type
```
An ordinary assignment such as `Config = {"name": str, "year": int}`, without
`type`, parses its right-hand side as an ordinary expression: it produces a
plain dict whose values happen to be type objects, it does not register
`Config` as a type alias, and its `{}` does not get TypedDict-shape
parsing. Which grammar applies is always visible at the point where a name is
bound, rather than depending on where the name is used later.

## Recursive type aliases

A `type` alias may refer to itself within its own definition. Resolving a
type expression is deferred until the name is actually used, the same way
imports are lazy, so a self-reference inside the alias body is not a
forward-reference problem the way it would be for an ordinary, eagerly
evaluated assignment:

```python
type PyTree[L] = L | list[PyTree[L]] | dict[str, PyTree[L]]

leaves: PyTree[int] = [1, {"a": 2, "b": [3, 4]}, 5]
```
Recursion is what makes a type like `PyTree` expressible at all: at every
level, the shape is either a leaf, or one of the listed containers holding
that very same shape one level down.

## Match types

A recursive alias like `PyTree` picks its shape by union — every
alternative is listed once, up front. Sometimes the type to produce
depends on the *structure* of another type instead: whether a list is
nested another level deeper, or already down to its leaf. A match type
is a `type` alias whose right side is `match`, reusing the same
`match`/`case` grammar [Exhaustive pattern matching](control-flow.md)
already has, computing a type from a type instead of a value from a
value:

```python
type Elem[T] = match T:
    case list[Inner]: Elem[Inner]
    case Leaf: Leaf
```
`Elem[list[list[int]]]` is `int`: each case peels off one layer of
nesting and recurses, until what is left is not a `list` at all.

Value-level `match` is a statement, deliberately without a case-body-
as-implicit-expression form, because that would make an ordinary
statement block secretly double as a value some of the time. A match
type has no such ambiguity to guard against: it lives entirely in the
type-expression grammar [Type expressions and the type keyword](#type-expressions-and-the-type-keyword)
already describes, and every case's only job, ever, is to produce one
type — so each case is a bare type expression, no block form needed.
Pattern capture follows the same convention Python's own structural
pattern matching already uses: a name that already names something is an
exact match (`case list[Inner]:` matches the shape `list` exactly),
and a name that doesn't is a fresh capture, bound to whatever the
subject actually was for use on the right (`Inner`, or `Leaf` for
whatever falls through to the second case).

A captured name can itself be a union — a subject typed `A | B` makes
any fresh capture over it a capture of `A | B` as a whole — and a
fresh-capture case runs once per member of a union subject, unioning the
results, the same distribution TypeScript's own conditional types
already do. This is what lets a recursive match type work through a
branching structure instead of getting stuck treating the whole union as
one opaque type: [Promotion](dispatch.md), later in the reading
order, puts exactly this to use for a type that branches in more than
one direction. Combined with `Never` already
being union's identity element — a type with no values contributes
nothing to a union, so `Never | X` is just `X`, true of any bottom
type in any type system that has one — distribution gives an
existential test for free:
if a fresh-capture case produces `Never` down every branch, the
distributed result is `Never`; if it produces something else down even
one branch, that survives, since every `Never` alongside it disappears.

## Literal types

`Literal[value]` is the type whose only inhabitant is that exact value —
using an ordinary value as a type, the opposite direction from `type
<type expression>` (above), which turns a type into an ordinary value.

`none` already relies on this every time it appears in a union like
`Item | none`: that `none` means the type containing only the `none`
singleton, which is exactly `Literal[none]`, written without the wrapper
because `none` is common enough to earn the shorthand.

Every other literal earns the same shorthand, not just `none`: a
bare integer, string, boolean, float, or complex literal written in a
type position already means `Literal[value]`, with nothing to
spell out:

```python
mode: 1 | 2 | 3
status: "ok" | "error"
flag: True
```
means exactly:

```python
mode: Literal[1, 2, 3]
status: Literal["ok", "error"]
flag: Literal[True]
```
None of this is ambiguous with what `1 | 2 | 3` means as a value,
because an annotation is already parsed as a type expression
([Type expressions and the type keyword](#type-expressions-and-the-type-keyword)), not an ordinary one — the
same separation that already lets `&`, `|`, and `not` mean
something different there than they do in a value. The shorthand is
for a literal written fresh, not for every singleton value a name
happens to refer to: `iteration.done` still needs the wrapper
spelled out, since it names an existing value rather than writing one:

```python
type IterResult[T] = T | Literal[iteration.done]
```
Python's own `Literal` cannot hold a `float` or `complex` value
at all — PEP 586 restricts it to `None`, `int`, `bool`, `str`,
`bytes`, and enum members, so a checker wanting `1.5` as a type has
to widen it to plain `float` and lose the precision, or reach for a
project-specific opt-in to get it back. Lucid's `Literal` has no such
restriction — a float or complex literal promotes exactly like any
other, with nothing to opt into.

## Constructor calls infer as `final`

A call naming the class it constructs can only ever produce an
instance of exactly that class — building a subclass instead needs its
own constructor call, `B()`, not `A()`. Lucid infers this
precisely: `A()`'s type is `final A`, not plain `A`:

```python
class A: ...

a = A()   # final A
```
This is the constructor counterpart of literal inference: `1` infers
as `Literal[1]` and widens to `int` wherever a declaration governs
it, and `final A` widens to `A` in exactly the same places:

```python
class B(A): ...

class C:
    x: A = A()

def g(c: C):
    c.x = B()   # ok — C.x's declared type is A, not final A

items: list[A] = [A()]   # list[A], not list[final A]
```
The extra precision buys disjointness. A value whose class is exactly
`A` cannot also be a `str`, and cannot be some subclass of `A`
either, so both possibilities narrow away — which is what lets the
non-overlapping check in [Identity and instance checks](control-flow.md) catch a test that can never hold, not just
against an unrelated type but against `A`'s own subclasses:

```python
a = A()
if a is B: ...   # error: an exactly-A value is never a B
```
## No `Any` escape hatch

Python's `Any` turns off type checking for a value entirely: nothing about
it is verified, any method can be called on it, and it can be assigned to or
from anything with no proof required. Lucid has no equivalent, and does not
need one — there is no legacy untyped Lucid code for a new, fully-typed
language to interoperate with, which is the pressure that makes `Any`
earn its place in a gradually-typed language like Python.

`object` already covers "I don't know or care what this is" the sound
way: anything is assignable to an `object`-typed slot, but only what
`object` itself promises — equality, hashing, representation — can be
called on one, until it is narrowed back to something more specific. That
is exactly the check `Any` skips.

`any Trait` (see [Existential types](generics.md)) looks similar and is not: it
erases which concrete type implements `Trait`, not whether the value
is checked at all. A value of type `any Functor[X]` is fully verified
against everything `Functor` declares — nothing more, nothing less — the
same guarantee `object` gives, just narrower. Composing the two, `any
object`, does not produce anything like `Any` either: it existentially
quantifies over the weakest possible bound, which adds nothing beyond what
`object` already provides on its own.

The one place something `Any`-shaped is actually needed is not inside
Lucid at all: a foreign, untyped value crossing the Python interop boundary
has to be accepted somehow, without static proof. That is a property of
whatever crosses that specific boundary, not a gap in the type system that
needs a general escape hatch to fill.

## Python interop and `trust`

A Python value crossing into Lucid with no further information is typed as
`object` — nothing is assumed about it, the same as any other value whose
shape is genuinely unknown. That is not a new rule; it is the no-`Any`
principle above applied to values that happen to come from outside Lucid,
instead of values that happen to be under-specified inside it.

Getting anything more specific out of an `object` that actually came from
Python requires an explicit, unverified claim: `trust`.

```python
raw: object = some_python_function()
items: !list[int] = trust[!list[int]](raw)
```
`trust` asserts a type with no proof behind it — there is nothing on the
Python side for the checker to verify against — but the claim is visible,
written once, at the exact place it is made. That is the difference from
`Any`: `Any` lets a value be used as anything, anywhere, with no marker
recording that a leap was taken; `trust` requires writing down exactly
what is being trusted, and where, every time.

`trust` only accepts an `object`-typed operand. Python's `typing.cast`
has no such restriction — it can assert any type in place of any other,
anywhere, purely between values that are already fully typed on the Python
side, with nothing foreign involved at all. Lucid has no equivalent
general-purpose `cast`, on purpose: traits are nominal specifically so
that satisfying one is an explicit, checked act rather than an accidental
shape match, and an unrestricted cast would let any code route around that
check between two ordinary, already-sound Lucid values — `trust[Dog](some_cat)`
between two well-typed Lucid values is not filling a real gap, it is
punching a hole where the checker already had real information. Restricting
`trust` to `object` operands makes that impossible by construction: it
only ever gets to speak where the checker had nothing to say in the first
place, which is exactly and only the Python interop boundary.

Calling `trust` at every use site does not scale to a whole library.
Attaching a claim once, at the import, the way a `.pyi` stub does for
Python's own type checkers, is the natural next step — where such a stub
would live, and how it interacts with lazy imports, is not yet decided.


