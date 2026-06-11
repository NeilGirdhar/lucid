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

Lucid adds member and module keywords including ``export``, ``lazy``, ``factory``, ``construct``, ``getter``, ``setter``, ``interface``, ``trait``, ``declare``, and ``frozen``.

2.4. Literals
-------------

Collection literals keep empty sets and empty dictionaries distinct.

.. code-block:: python

   empty_set = {}
   empty_dict = {:}
   names = {"Ada", "Grace"}
   scores = {"Ada": 10, "Grace": 9}

``{}`` constructs a set. ``{:}`` constructs a dictionary. A braced literal with key-value pairs is also a dictionary.
