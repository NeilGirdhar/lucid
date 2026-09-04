Control flow and statements
==============================

.. contents:: Table of contents
   :depth: 2
   :local:

Explicit boolean tests
--------------------------

Boolean tests require ``bool`` or explicit truth behavior. This applies to
``if``, ``while``, and other conditional control-flow positions.

Explicit iteration
----------------------

``for`` loops require an explicitly iterable value. A type is iterable only if
it implements or inherits from ``Iterable``.

Advancing an iterator returns a result, the same way any other recoverable
outcome does (see `Errors: results and exceptions`_): either the next
value, or a sentinel meaning there isn't one. Python signals this by
raising ``StopIteration`` — using an exception for the single most
ordinary outcome an iterator has, not an exceptional one. Lucid's iterators
return a value instead:

.. code-block:: python

   import iteration

   type IterResult[T] = T | Literal[iteration.done]

   interface Iterator[+T]:
       def __next__(self) -> IterResult[T]

``iteration.done`` is a singleton value, not a class — the same shape as
``none``, just not common enough to earn ``none``'s bare-word exemption in
type position, so it is written through ``Literal`` (see
`Literal types <types.rst>`_) instead of getting a type invented to
represent it. It matches like any other case:

.. code-block:: python

   match cursor.__next__():
       case Literal[iteration.done]:
           ...
       case _:
           ...

``iteration.done`` lives in the ``iteration`` module rather than being a
builtin the way ``none`` is: ``none`` is a genuinely universal absence of a
value, needed everywhere, while ``iteration.done`` only matters to code
that implements or consumes the iterator protocol directly — a small
enough audience that an explicit import is the right cost, not a builtin
every program pays for.

No loop ``else``
--------------------

Python loop ``else`` clauses run when a loop finishes without ``break``. Lucid
removes loop ``else``.

``if_broken`` loop clauses
-------------------------------

Lucid adds an optional ``if_broken`` clause for loops. ``if_broken`` is a
keyword. The ``if_broken`` suite runs if the loop exits by ``break``. Ordinary
fall-through code handles the case where a loop exits normally without
``break``.

.. code-block:: python

   def find_match(items: Iterable[Item]) -> Item | none:
       for item in items:
           if is_match(item):
               found = item
               break
       if_broken:
           return found
       return none

Nested ``if_broken`` clauses
----------------------------------

In nested loops, an ``if_broken`` clause belongs to the loop immediately before
it. A ``break`` inside an inner loop can therefore be handled by the inner
loop's ``if_broken`` clause, and that clause can choose to break the outer loop:

.. code-block:: python

   for row in rows:
       while has_more(row):
           if matches(row.current):
               break
       if_broken:
           break

Exhaustive pattern matching
-----------------------------------

Python's ``match``/``case`` cannot check that every case is covered: Python
classes are open, so there is no way to enumerate every shape a value could
have, and a non-exhaustive match with no matching case just does nothing at
runtime. Lucid's recursive union aliases are closed — every alternative is
named in one place — so the same check Rust and Swift already give their
closed enums is possible here, and Lucid does it:

.. code-block:: python

   type PyTree[L] = L | list[PyTree[L]] | dict[str, PyTree[L]]

   def tree_map(tree: PyTree[Array], f: Callable[[Array], bool]) -> PyTree[bool]:
       match tree:
           case Array:
               return f(tree)
           case list[PyTree[Array]]:
               return [tree_map(item, f) for item in tree]
           case dict[str, PyTree[Array]]:
               return {k: tree_map(v, f) for k, v in tree.items()}

The three cases are exactly ``PyTree``'s three alternatives, and that is
checked: leaving one out is an error, not a silent no-op the way an
incomplete Python ``match`` is. If ``PyTree`` ever grows a fourth
alternative, every ``match`` over it that lacks a matching case becomes an
error at the point it stops being exhaustive, rather than a bug found later
at runtime.

A ``case`` pattern is just a type — ``case Type:`` narrows the subject to
``Type`` for that case, using the subject's own name, without introducing
any pattern grammar of its own. That only works when the subject already is
a name, the way ``tree`` is above. When it is some other expression — a
call, an attribute access, anything without a name of its own to reuse —
``match`` needs one supplied, with ``as``:

.. code-block:: python

   match parse(text) as outcome:
       case ParseError:
           ...
       case Value:
           ...

``as outcome`` names the subject itself, for the cases to narrow and refer
to — it does not name a value the ``match`` produces, since ``match``
does not produce one. ``as`` is required whenever the subject is not
already a bare name, and has nothing to do otherwise: writing
``match tree as t:`` to rename an already-bare subject buys nothing that
``match tree:`` did not already have.

A bare ``_`` matches anything, satisfying exhaustiveness for a match over a
type that is not a closed union at all. Matching against a non-closed type
— ``object``, an interface, anything without a known, finite set of
alternatives — cannot be checked for exhaustiveness the way ``PyTree`` can,
and requires an explicit ``case _:`` for the same reason a Rust ``match``
over an integer needs one: there is no finite set of cases to exhaust.

``match`` is a statement, not an expression, and each ``case`` is an
ordinary statement block — any sequence of statements, exactly like the
body of an ``if``, a loop, or a function — not a single expression. It is
purely the exhaustive, type-narrowing form of an ``if``/``elif`` chain,
nothing more. Producing a value from it works exactly the way producing a value
from an ``if``/``elif`` chain already does: ``return``, as above, or an
assignment repeated in every branch. That repetition is a real, known cost,
not an oversight — the alternative was a case body that is secretly only
ever a single implicit-value expression, which nothing else in the language
does, and which is worse than the repetition it would save.

Pattern matching and dispatch solve related problems differently, and the
choice between them is the same one type theory calls the expression
problem. ``match`` requires every alternative in one place and checks that
nothing is missing; it is the right tool when the whole set of shapes is
closed and known ahead of time. `Dispatch beyond operators <dispatch.rst>`_
builds the same kind of function the opposite way: an open, growing set of
independently-checked cases that anyone can add to later, with no
exhaustiveness check possible, because the set is never closed. A fixed
``PyTree`` with two known container shapes is exactly matched to ``match``;
a version meant to stay open to third parties (see
`Existential types <types.rst>`_) is exactly matched to dispatch instead.

Errors: results and exceptions
-----------------------------------

Python collapses two different kinds of failure into one mechanism.
``raise``/``try``/``except`` handle both "this call can fail in an
ordinary, expected way" — a missing key, a parse failure — and "something
is broken" — a violated invariant, a bug — and because exceptions carry no
signature, nothing about a function's type distinguishes the two. Java
tried to fix the visibility problem by making exceptions checked —
declared in ``throws``, enforced by the compiler — and got the goal right
and the mechanism wrong: checked exceptions do not compose with generics
or lambdas, and one new exception type deep in a call chain forces every
intermediate signature to change, which is why nearly every language
designed since has rejected the mechanism while still agreeing with what
it was reaching for.

Lucid splits the two kinds of failure instead of choosing one mechanism for
both.

Recoverable errors are ordinary return types
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

An expected, recoverable failure is just part of what a function returns —
a union, handled the same exhaustive way any other union is:

.. code-block:: python

   type ParseResult = Value | ParseError

   def parse(text: str) -> ParseResult:
       ...

   match parse(text):
       case Value:
           ...
       case ParseError:
           ...

This needs no new mechanism: it is `Exhaustive pattern matching`_ applied
to errors, not a separate error-handling feature. The checker already
refuses to let a case go unhandled, which is the guarantee Java's checked
exceptions were reaching for, without the signature-propagation cost —
adding a new failure mode to ``ParseResult`` is an ordinary union change,
not something every caller up the chain has to redeclare.

The ``?`` operator
~~~~~~~~~~~~~~~~~~~~~

Matching every fallible call by hand does not scale. Go's version of this
problem is ``if err != nil { return nil, err }`` repeated after nearly
every call — legible, but the boilerplate outweighs the logic it surrounds.
``?`` is sugar for the common case: propagate the error case unchanged, or
unwrap to the value and keep going.

.. code-block:: python

   def load(path: str) -> Config | ParseError:
       text = read_file(path)?
       return parse(text)?

``text = read_file(path)?`` means exactly:

.. code-block:: python

   match read_file(path) as outcome:
       case ParseError:
           return outcome
       case _:
           text = outcome

``?`` requires the enclosing function's own return type to already accept
whatever error type is being propagated — ``load``'s return type has to
name ``ParseError`` for either ``?`` above to be legal, the same
requirement Rust's ``?`` places on the enclosing function's error type.

This is where Lucid lands between Python and Rust, and closer to Rust.
Python's exceptions propagate automatically with no mark anywhere in the
source that a given call might fail — reading a function's signature tells
you nothing about what it might raise. Lucid's ``?`` also propagates
automatically, but every place it happens is a literal character in the
source, visible and searchable, the same as Rust's — nothing is invisible:
the return type says what can go wrong, and each ``?`` says exactly where
a failure is allowed to end the current function early. That is strictly
more than Python gives you, for less ceremony than Go's manual check.

Unrecoverable errors keep ``raise``
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``raise``/``try``/``except``/``finally`` stay, narrowed to the other kind
of failure: a broken invariant, a bug, something that should never happen
— Rust's ``panic!``, not Rust's ``Result``. They are not for everyday,
expected outcomes the way Python's ``StopIteration``-driven iteration or
its ``KeyError``-then-catch idiom use them.

``raise`` stays unchecked — not declared in a function's signature, not
enforced by the checker. That is deliberate, not an oversight: the
visibility Java wanted from checked exceptions is already delivered by
recoverable errors being ordinary return types. Checking the broken-
invariant case too would just be ceremony around something no caller is
meant to routinely handle in the first place.

Unspecified simple statements
-----------------------------------

This sketch has not yet specified Lucid's full behavior for ``assert``,
``pass``, ``del``, ``break``, or ``continue``.
The ``skip`` keyword is an expression-level elision marker, not a replacement
for the statement-level ``pass`` placeholder.
