Strings and collections
==========================

.. contents:: Table of contents
   :depth: 2
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

Python treats ``str`` as ``Sequence[str]`` for static typing. Lucid keeps
``str`` narrower: it is ``Container[str]`` and ``Sized``, but not
``Iterable[str]``, ``Collection[str]``, or ``Sequence[str]``. Strings do not
provide ``__iter__``. This catches accidental character-by-character behavior
where a function asks for general strings or general sequences:

.. code-block:: python

   def render_lines(lines: Iterable[str]) -> str:
       return "\n".join(lines)

   render_lines("hello")  # error: str is not Iterable[str]

Code that wants a character sequence asks for the ``chars`` property explicitly.
``chars`` returns a read-only sequence view, ``Sequence?[str]``:

.. code-block:: python

   text: str = "hello"

   for ch in text:          # error
       ...

   chars: Sequence?[str] = text.chars
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

.. code-block:: python

   empty_set = {}
   empty_dict = {:}
   empty_frozenset = !{}
   empty_frozendict = !{:}
   names = {"Ada", "Grace"}
   scores = {"Ada": 10, "Grace": 9}
   immutable_names = !{"Ada", "Grace"}
   immutable_scores = !{"Ada": 10, "Grace": 9}

TypedDict shapes in type position
-------------------------------------

In a type expression (see the ``type`` keyword in
`Types, mutability, and annotations <types.rst>`_), a brace literal maps
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
call arguments. In list, tuple, and set literals, an entry that reaches
``skip`` is omitted:

.. code-block:: python

   [1, 2, 3 if false else skip, 4] == [1, 2, 4]

In dictionary literals, an entry is omitted if either the key expression or the
value expression reaches ``skip``:

.. code-block:: python

   {1: 2, 3: skip, skip: 6} == {1: 2}
