Type vocabulary
====================================

.. contents:: Table of contents
   :depth: 2
   :local:

What ``type`` means in Lucid
-------------------------------

Python overloads the word "type." The builtin ``type`` returns a value's
class; ``type[X]`` in an annotation means specifically a class object; and
"type" used loosely in "type annotation" or "type checker" means something
broader than either — any type expression, class or not. Static type
checkers eventually needed a name for that broader sense on its own:
``TypeForm`` — a type expression, evaluated to a value, whether or not it
happens to be a single class.

Lucid keeps that distinction, but gives each sense its own name instead of
overloading one word for both. A Lucid ``class`` is the narrow sense — the
direct analogue of a Python class: nominal, data-owning, producing instances
through construction. A Lucid *type* is the broad sense — the analogue of
``TypeForm``, not of ``class`` or ``type[X]``. Most Lucid types are not
classes at all: ``int | none``, ``!list[int]``, ``dict[str, int]``, an
anonymous record ``(x: int, y: int)``, and a TypedDict shape ``{...}`` are
all types with nothing that could be called a class behind them — type
expressions evaluated to values, the same way ``type <type expression>``
(see below) turns any of them into a first-class *type form* to pass around
like any other value.

Every ``class`` is a type. Not every type is a ``class``.

The word itself appears in exactly these roles in Lucid, and no others:

- **The category** — "a type," used the way this section has been using it:
  any type expression evaluated to a value, class or not. This is prose
  vocabulary, not syntax.
- **Type expression**, as a grammar — the parsing mode that applies in
  annotation positions, generic parameter lists, and interface member
  signatures (see `Type expressions and the type keyword`_), as opposed to
  the ordinary expression grammar used everywhere else. Also not syntax by
  itself — it names which grammar is in effect at a given position.
- ``type Name = <type expression>`` — the alias statement, giving a type
  expression a name usable in future type positions.
- ``type <type expression>`` — the prefix operator, reifying a type
  expression into an ordinary value — a type form — usable in ordinary
  expression positions.

Only the last two are actual keyword uses; both are the same keyword,
disambiguated by position rather than by two different words the way
Python's ``type``/``type[X]``/``TypeForm`` are.

Visible type contracts
-------------------------

Lucid's annotation syntax is Python's, unchanged: a colon after a name
annotates a field, a parameter, or a local variable, and ``->`` annotates a
function's return value. Interface requirements are annotated the same way
(see `Modern type specification <type-specification.rst>`_).

.. code-block:: python

   class User:
       name: str
       tags: set[str] = {"draft"}

   def greet(user: User, times: int = 1) -> str:
       lines: list[str] = [f"Hello, {user.name}"] * times
       return "\n".join(lines)

Type expressions and the ``type`` keyword
---------------------------------------------

Wherever a type is expected — variable, parameter, and return annotations,
generic parameter lists, interface member signatures — Lucid parses a *type
expression* rather than an ordinary expression. Most syntax means the same
thing in both grammars (``dict[str, int]``, ``&T``, ``!T``, and
``Producer[+K]`` all evaluate identically either way), but a type expression
can use forms that mean something else, or nothing at all, as an ordinary
expression — for example the TypedDict shape literal in
`Strings and collections <collections.rst>`_.

The ``type`` keyword crosses between the two grammars, in the two directions
that matter:

``type Name = <type expression>``
    A type alias statement. The right-hand side is parsed as a type
    expression, and ``Name`` becomes usable in future type positions exactly
    as if the aliased expression had been written inline there.

``type <type expression>``
    A prefix operator usable inside an ordinary expression. It parses its
    operand as a type expression and evaluates it to a first-class *type
    form*: an ordinary value, usable wherever ordinary values are, for example
    when passing a type to a metaprogramming function.

.. code-block:: python

   type Shape = !InferenceModel[str]
   value: Shape = freeze(model)

   form = type list[str]                                # an ordinary value: a reified type
   handlers = {"json": JSONHandler, "xml": XMLHandler}   # an ordinary dict, not a type

An ordinary assignment such as ``Shape = {"name": str, "year": int}``, without
``type``, parses its right-hand side as an ordinary expression: it produces a
plain dict whose values happen to be type objects, it does not register
``Shape`` as a type alias, and its ``{}`` does not get TypedDict-shape
parsing. Which grammar applies is always visible at the point where a name is
bound, rather than depending on where the name is used later.

Recursive type aliases
-----------------------------

A ``type`` alias may refer to itself within its own definition. Resolving a
type expression is deferred until the name is actually used, the same way
imports are lazy, so a self-reference inside the alias body is not a
forward-reference problem the way it would be for an ordinary, eagerly
evaluated assignment:

.. code-block:: python

   type PyTree[L] = L | list[PyTree[L]] | dict[str, PyTree[L]]

   leaves: PyTree[int] = [1, {"a": 2, "b": [3, 4]}, 5]

Recursion is what makes a type like ``PyTree`` expressible at all: at every
level, the shape is either a leaf, or one of the listed containers holding
that very same shape one level down.

Literal types
-----------------------------

``Literal[value]`` is the type whose only inhabitant is that exact value —
using an ordinary value as a type, the opposite direction from ``type
<type expression>`` (above), which turns a type into an ordinary value.

``none`` already relies on this every time it appears in a union like
``Item | none``: that ``none`` means the type containing only the ``none``
singleton, which is exactly ``Literal[none]``, written without the wrapper
because ``none`` is common enough to earn the shorthand. Other singleton
values do not get that exemption and are written out in full:

.. code-block:: python

   type IterResult[T] = T | Literal[iteration.done]

No ``Any`` escape hatch
-----------------------------

Python's ``Any`` turns off type checking for a value entirely: nothing about
it is verified, any method can be called on it, and it can be assigned to or
from anything with no proof required. Lucid has no equivalent, and does not
need one — there is no legacy untyped Lucid code for a new, fully-typed
language to interoperate with, which is the pressure that makes ``Any``
earn its place in a gradually-typed language like Python.

``object`` already covers "I don't know or care what this is" the sound
way: anything is assignable to an ``object``-typed slot, but only what
``object`` itself promises — equality, hashing, representation — can be
called on one, until it is narrowed back to something more specific. That
is exactly the check ``Any`` skips.

``any Interface`` (see `Existential types <generics.rst>`_) looks similar and is not: it
erases which concrete type implements ``Interface``, not whether the value
is checked at all. A value of type ``any Functor[X]`` is fully verified
against everything ``Functor`` declares — nothing more, nothing less — the
same guarantee ``object`` gives, just narrower. Composing the two, ``any
object``, does not produce anything like ``Any`` either: it existentially
quantifies over the weakest possible bound, which adds nothing beyond what
``object`` already provides on its own.

The one place something ``Any``-shaped is actually needed is not inside
Lucid at all: a foreign, untyped value crossing the Python interop boundary
has to be accepted somehow, without static proof. That is a property of
whatever crosses that specific boundary, not a gap in the type system that
needs a general escape hatch to fill.

Python interop and ``trust``
-----------------------------------

A Python value crossing into Lucid with no further information is typed as
``object`` — nothing is assumed about it, the same as any other value whose
shape is genuinely unknown. That is not a new rule; it is the no-``Any``
principle above applied to values that happen to come from outside Lucid,
instead of values that happen to be under-specified inside it.

Getting anything more specific out of an ``object`` that actually came from
Python requires an explicit, unverified claim: ``trust``.

.. code-block:: python

   raw: object = some_python_function()
   items: !list[int] = trust[!list[int]](raw)

``trust`` asserts a type with no proof behind it — there is nothing on the
Python side for the checker to verify against — but the claim is visible,
written once, at the exact place it is made. That is the difference from
``Any``: ``Any`` lets a value be used as anything, anywhere, with no marker
recording that a leap was taken; ``trust`` requires writing down exactly
what is being trusted, and where, every time.

``trust`` only accepts an ``object``-typed operand. Python's ``typing.cast``
has no such restriction — it can assert any type in place of any other,
anywhere, purely between values that are already fully typed on the Python
side, with nothing foreign involved at all. Lucid has no equivalent
general-purpose ``cast``, on purpose: interfaces are nominal specifically so
that satisfying one is an explicit, checked act rather than an accidental
shape match, and an unrestricted cast would let any code route around that
check between two ordinary, already-sound Lucid values — ``trust[Dog](some_cat)``
between two well-typed Lucid values is not filling a real gap, it is
punching a hole where the checker already had real information. Restricting
``trust`` to ``object`` operands makes that impossible by construction: it
only ever gets to speak where the checker had nothing to say in the first
place, which is exactly and only the Python interop boundary.

Calling ``trust`` at every use site does not scale to a whole library.
Attaching a claim once, at the import, the way a ``.pyi`` stub does for
Python's own type checkers, is the natural next step — where such a stub
would live, and how it interacts with lazy imports, is not yet decided.

