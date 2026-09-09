# Type vocabulary

## What `class` means in Lucid

Python's builtin `type` returns a value's class, and `type[X]` in an
annotation means specifically a class object — two of the three things
Python overloads onto one word. A Lucid `class` is that narrow sense
given its own name: the direct analogue of a Python class — nominal,
data-owning, producing instances through construction.

The annotation follows the same renaming: where Python writes `type[X]`
for the type of `X`'s own class object, Lucid writes `class[X]` — the
keyword that already names the narrow sense, used the same way any other
generic name is:

```python
def register(handler: class[Handler]) -> none:
    ...

register(JSONHandler)     # ok: a class object
register(JSONHandler())   # error: an instance, not a class object
```

## What `type` means in Lucid

The third sense Python overloads onto the same word is looser still:
"type" used in "type annotation" or "type checker" means something
broader than a class — any type expression, class or not. Static type
checkers eventually needed a name for that broader sense on its own:
`TypeForm` — a type expression, evaluated to a value, whether or not it
happens to be a single class.

Lucid gives this sense its own name too, instead of overloading one
word for both: a Lucid *type* is the broad sense — the analogue of
`TypeForm`, not of `class` or `type[X]`. Most Lucid types are not
classes at all: `int | none`, `!list[int]`, `dict[str, int]`, an
anonymous record `(x: int, y: int)`, and a TypedDict shape `{...}` are
all types with nothing that could be called a class behind them — type
expressions evaluated to values, the same way `type <type expression>`
(see below) turns any of them into a first-class *type form* to pass around
like any other value.

Every `class` is a type. Not every type is a `class`.

The word itself appears in exactly these roles in Lucid, and no
others — only the last two are actual keyword uses, the same keyword
disambiguated by position rather than by two different words the way
Python's `type`/`type[X]`/`TypeForm` are:

- **The category** — "a type," used the way this section has been using
  it: any type expression evaluated to a value, class or not. This is
  prose vocabulary, not syntax.
- **Type expression**, as a grammar — covered next.
- `type Name = <type expression>` — the alias statement, covered in
  [Type aliases](#type-aliases).
- `type <type expression>` — the prefix operator, covered in
  [Reifying a type expression](#reifying-a-type-expression).

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
— see [Anonymous class](anonymous-class.md) for the full shape.

This form is recognized only in type positions, the same restriction
already placed on other type-only syntax: `(int) -> int` written where
a value is expected is an ordinary parenthesized expression, not a
callable literal. Lucid also has no gradual, any-arity form — Python's
`Callable[..., R]` — since that is exactly the `Any`-shaped escape
hatch this document already closes off for everyday code; a genuinely
unknown foreign signature stays `object`, claimed with `trust` like
any other untyped value crossing the interop boundary.
[Operations](type-operations.md) covers the ways types combine —
union, intersection, and negation.

## The `Callable` trait

Every function value satisfies `Callable` automatically — the
compiler marks it, not a class header a function could forget to
declare, the same way every value already satisfies `object` without
declaring it. `x is Callable` replaces Python's bare `callable(x)`,
the same swap [Identity and instance checks](identity-checks.md)
already makes for `isinstance`/`issubclass`:

```python
def apply(x: object) -> none:
    if x is Callable:
        x()
```

`Callable` carries no parameter or return information — that is
exactly what `(A, B) -> R` already spells out precisely, and what the
rejected, gradual `Callable[..., R]` form would have thrown away
([Gradual, any-arity callable type](rejected-features.md#gradual-any-arity-callable-type-python-basedpython)).
`Callable` only answers whether a value is callable at all, the same
narrow job `is class` answers for classes
([`is trait` and `is class`](identity-checks.md#is-trait-and-is-class)) —
a bare capability check, never a substitute for a real signature.

## Type expressions

Wherever a type is expected — variable, parameter, and return annotations,
generic parameter lists, trait member signatures — Lucid parses a *type
expression* rather than an ordinary expression. Most syntax means the same
thing in both grammars (`dict[str, int]`, `~T`, `!T`, and
`Producer[+K]` all evaluate identically either way), but a type expression
can use forms that mean something else, or nothing at all, as an ordinary
expression — for example the TypedDict shape literal covered next.

## TypedDict shapes in type position

In a type expression, a brace literal maps literal keys to per-key types
instead of constructing a dict value. This is Lucid's TypedDict: an exact
dict shape, not a class. Values are ordinary dicts, indexed and iterated
like any other dict, but each key's value is checked against that key's
own type instead of every value being unified into one value type.

```python
type Movie = {"name": str, "year": int}

movie: Movie = {"name": "Paths of Glory", "year": 1957}
movie["year"] += 1
movie["name"] = 1957          # error: str expected
```
Keys are not restricted to strings. Any literal hashable key is allowed:

```python
type Row = {0: str, 1: int, "label": str}
```
A trailing `...` marks the shape open, allowing keys beyond the ones listed:

```python
type Movie = {"name": str, "year": int, ...}

movie: Movie = {"name": "Paths of Glory", "year": 1957, "director": "Kubrick"}
```
Outside a type expression, the same brace syntax is an ordinary dict literal:
written as a plain expression, `{"name": str, "year": int}` is a dict value
mapping to the `str` and `int` type objects, not a `Movie` shape.

## Type aliases

`type Name = <type expression>` is a type alias statement, the first of
the `type` keyword's two directions across the grammar boundary. The
right-hand side is parsed as a type expression, and `Name` becomes
usable in future type positions exactly as if the aliased expression
had been written inline there:

```python
type Config = !InferenceModel[str]
value: Config = freeze(model)
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
that very same shape one level down. [Operations](type-operations.md) covers
the other direction — combining and transforming types that already exist,
rather than assembling a new one from scratch.

## Reifying a type expression

`type <type expression>` is a prefix operator, the `type` keyword's
other direction across the grammar boundary — usable inside an
ordinary expression instead of a statement of its own. It parses its
operand as a type expression and evaluates it to a first-class *type
form*: an ordinary value, usable wherever ordinary values are, for
example when passing a type to a metaprogramming function:

```python
form = type list[str]                                # an ordinary value: a reified type
handlers = {"json": JSONHandler, "xml": XMLHandler}   # an ordinary dict, not a type
```

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
([Type expressions](#type-expressions)), not an ordinary one — the
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

## Final types

`final A` is the type of values whose class is exactly `A`, never a
subclass — the same "nothing wider exists" idea `final`'s
class-declaration sense already has, applied to one value's type
instead of a whole class's openness. A value most often gets this type
by inference, from a constructor call
([Constructor calls infer as final](construction.md)), but it is an
ordinary type in its own right, writable wherever any other type is:

```python
class A: ...
class B(A): ...

def f(a: final A, b: B):
    if a is B: ...   # error: an exactly-A value is never a B
```
The extra precision buys disjointness. A value of type `final A`
cannot also be a `str`, and cannot be some subclass of `A` either, so
both possibilities narrow away — which is what lets the non-overlapping
check in [Identity and instance checks](identity-checks.md) catch a test
that can never hold, not just against an unrelated type but against
`A`'s own subclasses. `final A` widens to plain `A` wherever a
declaration governs it — a field, a parameter, a collection element —
the same as any other narrower type widening to a broader declared one.

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
needs a general escape hatch to fill. [Casting](casting.md) covers the one
place Lucid does let a claim like that be made explicit.


