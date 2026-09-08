Main ideas
==========

Lucid is a Python-like language sketch that keeps Python easy to read and
write, poaches the best ideas other languages already found, and removes
the compatibility constraints that keep Python from adopting many of its
own best proposals.

Core principle:

    Keep Python's directness, make structure explicit, and choose the cleaner
    rule when compatibility no longer has to win.

.. contents:: Table of contents
   :depth: 2
   :local:

Zero-deprecation
--------------------

Python's five-year end-of-life cadence was already an improvement over
C++'s near-permanent backward compatibility, which spends decades
accumulating designs nobody can remove. Python still carries a smaller
version of the problem: a deprecation stays supported across several
versions before removal is possible, so old designs often outlive their
reasons.

Lucid takes this further: a one-year cadence, each release shipping LLM
instructions to upgrade existing code. There is no deprecation period — a
feature can change or vanish in the next release, since migrating is
mechanical, not a multi-year sunset to wait out.

This makes the freedom used throughout this specification a standing
property, not a founding choice: Python often can't adopt its own cleaner
proposals because programs depend on old behavior; Lucid treats rejected
or constrained ideas as open design space, since compatibility never has
to win by default. That freedom doesn't expire — it lasts as long as Lucid
keeps releasing, keeping the language efficient and relevant instead of
accumulating the debt zero-deprecation avoids.

For a full list of preserved, discarded, and new keywords, see
`Keyword reference <keywords.rst>`_.

Python readability
-------------------

Lucid code should stay as easy to read and write as ordinary Python:
indentation matters, definitions are direct, common control flow is
familiar, and simple programs need no ceremony. See
`Names, binding, and scope <names.rst>`_ and
`Control flow and statements <control-flow.rst>`_.

Succinct code
~~~~~~~~~~~~~~~~

Like Python, Lucid aims for simple, succinct code — two lines from the Zen
of Python (``import this``): "Simple is better than complex," and "There
should be one — and preferably only one — obvious way to do it."

One obvious way
~~~~~~~~~~~~~~~~~~

Python used to hold that line more closely: three generations of string
formatting (``%``-formatting, ``.format()``, f-strings) still coexist, and
a bag of named fields can be a plain class, a ``dataclass``, a
``NamedTuple``, or a ``TypedDict``.

`Zero-deprecation`_ is why Lucid keeps one way — `No tuple or namedtuple
type <collections.rst>`_ picked named records once, instead of letting
tuples, namedtuples, and dataclasses coexist.

No memorized patterns
~~~~~~~~~~~~~~~~~~~~~~~~

Lucid also avoids patterns that work only because you've memorized them: a
decorator silently strips a function's identity unless you remember
``functools.wraps``; forgetting ``__slots__`` lets a typo'd attribute
silently create new state instead of raising an error; an abstract method
needs both a base class that inherits from ``ABC`` and an
``@abstractmethod`` decorator on every member.

Lucid closes each by construction: ``@`` always preserves identity
(`Preserving identity <decorators.rst>`_); class shape is closed by
default, with nothing to opt into (`No undeclared fields <classes.rst>`_);
a bodyless trait member is already an obligation (`Traits
<traits.rst>`_).

Explicit over implicit
--------------------------

Python lets behavior happen somewhere other than where you're looking:
``__getattr__``, descriptors, and metaclasses intercept normal-looking
code; ``typing.Protocol`` grants conformance to code that never asked for
it; an unannotated generic parameter's variance is inferred from whatever
the class currently does.

Lucid closes each off: traits are nominal, so a promise is made only
where a class header names it (`No structural traits
<traits.rst>`_); ``final`` and ``override`` must be written, never
inferred (`Explicit overrides <traits.rst>`_); attribute access has no
interception hooks (`Classes <classes.rst>`_). Nothing about a piece of
code's behavior should depend on something declared elsewhere the reader
never saw.

Java-style single inheritance
---------------------------------

Python's multiple inheritance uses one mechanism — the base-class list —
for several different jobs at once: inheriting shared state, promising an
interface, providing reusable behavior, and building the MRO. Those jobs
interfere: MRO order can silently change which implementation a method
call reaches, cooperative ``super()`` only works if every class in the
chain agrees on a calling convention, and two state-owning parents can
leave no sensible way to construct the result.

Java already split this apart, deliberately, against C++'s diamond
problem: one class parent, any number of interfaces, each free to mix
required methods with reusable default ones. Lucid keeps exactly that
split, naming the non-state-owning kind ``trait``. A class may extend
at most one other class, since only a class owns stored state and only
state creates the collision; it can use any number of traits, since a
trait owns no state, whatever mix of obligations and reusable defaults
it declares. See `Traits <traits.rst>`__ and
`One class parent <classes.rst>`_.

Scala-style type information
------------------------------

Putting type relationships at the definition site instead of inferring
them makes each one a checked, versioned part of a type's public contract.
Variance inferred from usage can flip unintentionally — a method that
consumes a type parameter can turn an inferred covariant trait
invariant, breaking downstream code that never touched it. Mutability
views give the same guarantee for a different question: whether a
function can mutate what you hand it is visible in its signature, not
something to trust from a docstring.

Type relationships live where abstractions are defined. Generic parameters
carry definition-site variance with ``+K``, ``-K``, and ``=K``. Mutable,
read-only, and immutable views are visible in the type spelling with ``T``,
``~T``, and ``!T``. See `Generics <generics.rst>`_,
`Mutability <mutability.rst>`_, and
`Modern type specification <type-specification.rst>`_.

Julia-style dynamic dispatch
------------------------------

Python's binary operators are single-dispatch on the left operand, so a
second method and a negotiation protocol — ``__radd__`` and
``NotImplemented`` — exist only to approximate the two-sided decision
``a + b`` needs. Multiple dispatch makes operators ordinary functions that
pick an implementation from every argument's type at once, the same rule
that already governs any function call — no reflected-method pairs, no
negotiation protocol, no separate mental model. It also stays open to
third parties the way `Dispatch beyond operators <dispatch.rst>`_ already
is for ordinary functions.

Operations can dispatch on all relevant runtime argument types. Binary
operators are generic multiple-dispatch operations: they are not owned by the
left operand, and Lucid does not use reflected methods or ``NotImplemented``
as an operator negotiation protocol. See `Multiple dispatch <dispatch.rst>`_.

Rust-style error handling
------------------------------

Python collapses two different kinds of failure into one mechanism:
``raise``/``try``/``except`` handle both an expected, recoverable outcome —
a missing key, a parse failure — and a broken invariant that should never
happen, with nothing in a function's signature saying which kind it might
produce, or whether it can fail at all. Java's checked exceptions fixed
the visibility problem with the wrong mechanism: they don't compose with
generics or lambdas, and one new exception type deep in a call chain
forces every intermediate signature to change.

Lucid splits the two kinds instead of choosing one mechanism for both: a
recoverable failure is part of an ordinary return type, checked
exhaustively the same way any other union is, with ``?`` as sugar to
propagate it without Go's ``if err != nil`` boilerplate; ``raise`` stays,
narrowed to broken invariants, unchecked, since nothing about them is
meant to be routinely handled. See
`Errors: results and exceptions <control-flow.rst>`_.

Basedpython-style loop bindings
-----------------------------------

Python's ``for`` loop reuses one binding across every iteration, so a
closure created inside the loop body captures that shared variable
instead of the value it appeared to capture: ``fns = []; for i in [1, 2,
3]: fns.append(def(): print(i))`` prints ``3 3 3`` in Python, since every
closure shares the one binding the loop kept reassigning.

Lucid gives each iteration a fresh binding instead, following
``basedpython``: the closures above print ``1 2 3``, each one keeping the
value from the iteration that created it. See
`Fresh loop bindings <control-flow.rst>`_.

Kotlin-style function types
-------------------------------

Python spells a callable's type ``Callable[[A, B], R]``: two nested
brackets and a comma-separated list, inherited from having to fit a
parameter list inside the same square-bracket generic syntax as every
other type. It reads nothing like the ``def`` whose type it describes.

Lucid spells it ``(A, B) -> R``, matching Kotlin: the same ``->`` a
``def``'s own return type already uses, applied to the type of a
function instead of to one definition of it. A parameter list that needs
names, positional-only or keyword-only zones, or variadic gathering uses
the same grammar an ordinary signature already does, so the type and the
definition it describes are never spelled two different ways. See
`Function types <types.rst>`_.
