2. Lexical analysis
===================

Lucid uses Python-like lexical structure unless this reference says otherwise.

2.1. Line structure
-------------------

Lucid uses indentation to delimit blocks.

2.2. Other tokens
-----------------

Lucid reserves punctuation in type positions for mutability views:

.. code-block:: python

   T   # mutable/default
   T?  # read-only view
   T!  # immutable

2.3. Names, identifiers, and keywords
-------------------------------------

Lucid adds member and module keywords including ``export``, ``lazy``,
``factory``, ``construct``, ``getter``, ``setter``, ``interface``, ``trait``,
``declare``, ``frozen``, and ``skip``.

The single underscore ``_`` is also a keyword. It is a black-hole assignment
target, not a name binding.

``if_broken`` is a soft keyword. It acts as a keyword only when it introduces a
loop ``if_broken`` clause; elsewhere it remains an ordinary identifier.

2.4. Literals
-------------

Adjacent string literals are not concatenated. Two string literal tokens cannot
appear next to each other as one expression.

.. code-block:: python

   path = "/api/" "users"  # error
   path = "/api/" + "users"

Collection literals keep empty sets and empty dictionaries distinct.

.. code-block:: python

   empty_set = {}
   empty_dict = {:}
   empty_frozenset = !{}
   empty_frozendict = !{:}
   names = {"Ada", "Grace"}
   scores = {"Ada": 10, "Grace": 9}
   frozen_names = !{"Ada", "Grace"}
   frozen_scores = !{"Ada": 10, "Grace": 9}

``{}`` constructs a set. ``{:}`` constructs a dictionary. A braced literal with
key-value pairs is also a dictionary.

The ``skip`` keyword may appear in collection entries. In list, tuple, and set
literals, an entry that reaches ``skip`` is omitted from the result:

.. code-block:: python

   [1, 2, 3 if False else skip, 4] == [1, 2, 4]

In dictionary literals, an entry is omitted if either the key expression or the
value expression reaches ``skip``:

.. code-block:: python

   {1: 2, 3: skip, skip: 6} == {1: 2}

The immutable marker ``!`` before a collection literal constructs the immutable
variant. ``!{a, b}`` constructs a frozenset. ``!{a: b}`` constructs a
frozendict. ``!{}`` is an empty frozenset, and ``!{:}`` is an empty frozendict.
