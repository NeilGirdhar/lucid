Strings and collections
==========================

.. contents:: Table of contents
   :depth: 3
   :local:

No adjacent string literal concatenation
--------------------------------------------

Python concatenates adjacent string literals at compile time. Lucid rejects
adjacent string literals. Use an explicit concatenation operation when a string
is meant to be joined.

.. code-block:: python

   path = "/api/" "users"  # error
   path = "/api/" + "users"

Strings are not sequences
-----------------------------

Python's ``str`` satisfies ``Sequence[str]``, since a string is itself a
sequence of one-character strings. So a function asking for a sequence or
iterable of strings also accepts a single string, and nothing about that is
a type error:

.. code-block:: python

   def render_lines(lines: Sequence[str]) -> str:
       return "\n".join(lines)

   render_lines("hello")  # type-checks, returns "h\ne\nl\nl\no"

A caller who forgot to wrap a single string in a list gets an object that
type-checks and runs, silently producing garbled output instead of a caught
mistake. Lucid keeps ``str`` narrower: it is ``Container[str]`` and
``Sized``, but not ``Iterable[str]``, ``Collection[str]``, or
``Sequence[str]``. Strings do not provide ``__iter__``, so the same mistake
is a type error instead of a silent, wrong result:

.. code-block:: python

   def render_lines(lines: Iterable[str]) -> str:
       return "\n".join(lines)

   render_lines("hello")  # error: str is not Iterable[str]

Code that wants a character sequence asks for the ``chars`` property explicitly.
``chars`` returns a read-only sequence view, ``~Sequence[str]``:

.. code-block:: python

   text: str = "hello"

   for ch in text:          # error
       ...

   chars: ~Sequence[str] = text.chars
   chars[0]

   for ch in text.chars:
       ...

String containment and length do not require string iteration:

.. code-block:: python

   "e" in text
   len(text)

``{}`` is a set and ``{:}`` is a dictionary
-----------------------------------------------

Python uses ``{}`` for an empty dictionary and requires ``set()`` for an empty
set. Lucid uses ``{}`` for an empty set and ``{:}`` for an empty dictionary. A
braced literal with key-value pairs is also a dictionary.

Immutable collection literals
---------------------------------

The immutable marker ``!`` before a collection literal constructs the immutable
variant. ``!{a, b}`` constructs a frozenset. ``!{a: b}`` constructs a
frozendict. ``!{}`` is an empty frozenset, and ``!{:}`` is an empty frozendict.

A mutable set or dict cannot be hashed: its contents could change after
insertion, which would silently corrupt every hash-based container it was
placed in. A frozenset or frozendict has no such problem — it is hashable
whenever its own values are hashable — so it can be used anywhere a hashable
value is required, including as a dict key or as a member of another set:

.. code-block:: python

   empty_set = {}
   empty_dict = {:}
   empty_frozenset = !{}
   empty_frozendict = !{:}
   names = {"Ada", "Grace"}
   scores = {"Ada": 10, "Grace": 9}
   immutable_names = !{"Ada", "Grace"}
   immutable_scores = !{"Ada": 10, "Grace": 9}

   groups: dict[frozenset[str], int] = {immutable_names: 2}

No tuple or namedtuple type
---------------------------------

Lucid has no ``tuple`` type and no ``namedtuple``. Python uses a tuple for
two different jobs — a small hashable ordered sequence, and a lightweight
heterogeneous record — and conflating them costs more than it saves.

Why not positional records
~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Fixed-position fields are fragile as an API grows. A function returning a
2-tuple that later grows a third value breaks every positional unpacking
call site:

.. code-block:: python

   def bounding_box():
       return width, height          # 2-tuple

   w, h = bounding_box()

   def bounding_box():
       return width, height, depth   # grew to 3-tuple

   w, h = bounding_box()  # ValueError: too many values to unpack

Written with a named-field return type, the same growth is not a breaking
change:

.. code-block:: python

   def bounding_box() -> (width: int, height: int):
       return (width=2, height=3)

   bb = bounding_box()
   bb.width + bb.height

   def bounding_box() -> (width: int, height: int, depth: int):
       return (width=2, height=3, depth=1)

   bb = bounding_box()
   bb.width + bb.height  # still fine -- callers that never asked about depth don't need it

Two same-typed fields can also be silently swapped with no type error at
all, since position is the only thing that says which is which:

.. code-block:: python

   def stats():
       return mean, median

   m, med = stats()

   def stats():
       return median, mean  # reordered during a refactor — still type-checks

   m, med = stats()  # silently wrong: m is now the median

``namedtuple`` does not fix this: it still compares equal to a plain tuple,
and to an unrelated ``namedtuple`` of the same shape, because equality is
inherited from ``tuple`` and never looks at the type:

.. code-block:: python

   Point = namedtuple("Point", ["x", "y"])
   Color = namedtuple("Color", ["r", "g"])

   Point(1, 2) == Color(1, 2)  # True

A class instance sidesteps all of this: fields are read by name, not by
position or by unpacking (see `Unpacking`_ below). That is more extensible,
since a producer can add fields without breaking any caller who only reads
the fields they asked for; more legible, since every field is read by name
instead of by position; and simpler, since there is no separate
tuple-versus-list question to answer for every new piece of data.

Having no tuple type closes off an unrelated footgun for free.
``assert`` reads enough like a function call that wrapping its arguments
in parentheses, the way any other call gets formatted, is a natural
mistake:

.. code-block:: python

   assert (x == y, "x and y should match")   # always true in Python

A non-empty tuple is always truthy, so the assertion silently never
fires — a mistake so common Python's own linters specifically watch for
it. Lucid cannot reproduce it: ``(x == y, "x and y should match")``
would have to build a tuple to be silently truthy, and there is no
tuple type left to build one with.

Unpacking
~~~~~~~~~~~~

Multiple assignment still unpacks any ``Iterable``, positionally, the same
way Python does:

.. code-block:: python

   first, second = [1, 2]

A plain class is not ``Iterable`` (see
`No __getitem__ iteration fallback <indexing.rst>`_), so a class instance
cannot be unpacked this way — reading its fields by name is the only way
in. A starred target on the left-hand side collects the remaining elements
into a ``list``, not a tuple:

.. code-block:: python

   first, *rest = [1, 2, 3, 4]
   rest: list[int] = [2, 3, 4]

Lucid uses Python's operators and Python's order of operations unless this
document says otherwise. In an expression, as opposed to an assignment
target, unpacking binds tighter than binary operators:

.. code-block:: python

   *x + y

means:

.. code-block:: python

   (*x) + y

not:

.. code-block:: python

   *(x + y)

Hashable sequences
~~~~~~~~~~~~~~~~~~~~

For a hashable, immutable ordered sequence — the job a tuple is actually
suited for — use the immutable marker on a list literal, ``![...]``, the
same ``!`` that constructs a frozenset or frozendict:

.. code-block:: python

   point: !list[int] = ![3, 4]
   cache: dict[!list[int], float] = {:}
   cache[![3, 4]] = distance(3, 4)

Heterogeneous records
~~~~~~~~~~~~~~~~~~~~~~~

For a small heterogeneous set of named fields, use a class. Every class is
already a dataclass: fields are declared in the class body and a
field-based constructor comes for free (see
`Modern type specification <type-specification.rst>`_).

.. code-block:: python

   class Point:
       x: int
       y: int

Anonymous record shapes
~~~~~~~~~~~~~~~~~~~~~~~~~

A record's shape can also be written directly as a type, without declaring
a named class, using ``(...)`` with each field's name and type:

.. code-block:: python

   type Point2D = (x: int, y: int)

   def midpoint(a: Point2D, b: Point2D) -> Point2D:
       ...

``(x: int, y: int)`` is structural: any class with at least those fields at
those types satisfies it, the same way ``Point`` above would, with no
declared relationship required — the same way a plain dict satisfies a
TypedDict shape. The immutable marker applies here too: ``!(x: int, y: int)``
is the frozen version of the same shape.

Outside a type expression, the same ``(...)`` syntax constructs an anonymous
value of that shape directly, using ``=`` instead of ``:`` before each
field's value — no named class required:

.. code-block:: python

   origin: Point2D = (x=0, y=0)
   origin.x

Constructing with ``=`` instead of ``:`` is not just a style choice: it
means the construction site already reads like a call, so replacing the
anonymous shape with a named class later is a small edit, not a rewrite —
``(x=0, y=0)`` becomes ``Point2D(x=0, y=0)``.

TypedDict shapes in type position
-------------------------------------

In a type expression (see the ``type`` keyword in
`Type vocabulary <types.rst>`_), a brace literal maps
literal keys to per-key types instead of constructing a dict value. This is
Lucid's TypedDict: an exact dict shape, not a class. Values are ordinary
dicts, indexed and iterated like any other dict, but each key's value is
checked against that key's own type instead of every value being unified
into one value type.

.. code-block:: python

   type Movie = {"name": str, "year": int}

   movie: Movie = {"name": "Paths of Glory", "year": 1957}
   movie["year"] += 1
   movie["name"] = 1957          # error: str expected

Keys are not restricted to strings. Any literal hashable key is allowed:

.. code-block:: python

   type Row = {0: str, 1: int, "label": str}

A trailing ``...`` marks the shape open, allowing keys beyond the ones listed:

.. code-block:: python

   type Movie = {"name": str, "year": int, ...}

   movie: Movie = {"name": "Paths of Glory", "year": 1957, "director": "Kubrick"}

Outside a type expression, the same brace syntax is an ordinary dict literal:
written as a plain expression, ``{"name": str, "year": int}`` is a dict value
mapping to the ``str`` and ``int`` type objects, not a ``Movie`` shape.

``skip`` in collection literals
-----------------------------------

``skip`` is not a value and cannot be returned, assigned, or passed through
ordinary expressions. It is valid only inside elidable collection entries and
call arguments. In list and set literals, an entry that reaches
``skip`` is omitted:

.. code-block:: python

   [1, 2, 3 if false else skip, 4] == [1, 2, 4]

In dictionary literals, an entry is omitted if either the key expression or the
value expression reaches ``skip``:

.. code-block:: python

   {1: 2, 3: skip, skip: 6} == {1: 2}
