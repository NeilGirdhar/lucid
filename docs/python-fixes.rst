Problems with Python that Lucid fixes
=====================================

Lucid borrows Python's readable syntax, but changes the parts of Python where behavior can be hidden behind dynamic object-model hooks, scattered conventions, or checker-specific side channels.

Object shape
------------

Python instances usually have an open-ended ``__dict__``, unless a class uses ``__slots__``, dataclasses with options, extension types, or custom attribute hooks. Lucid makes fixed shape the default: stored fields are declared in the class body, and undeclared fields are rejected.

Use an explicit dictionary field when dynamic keys are part of the model.

Construction
------------

Python splits construction across ``__new__``, ``__init__``, dataclass-generated initializers, ``__post_init__``, ``InitVar``, and field options such as ``init=False`` or ``kw_only``.

Lucid has one construction model: factories return fully constructed objects through the factory-only ``construct`` keyword.

A Python backend can lower this however it needs to. For example, generated Python might give the class a private construction class method that allocates the object, assigns fields, and returns it, then compile Lucid factories into calls to that helper.

Type annotations
----------------

Python's type annotations are valuable, but they are layered over a language whose runtime object model remains open and whose generic variance is often hidden in library declarations. The result is that important API facts can feel external to the class, split between stubs, decorators, checker rules, and naming conventions.

Lucid follows Scala more closely here. Scala's type parameters sit directly on classes and traits, and variance markers such as ``+A`` and ``-A`` make subtyping behavior a property of the abstraction itself. Lucid adopts that clarity with ``+K``, ``-K``, and ``=K``. The useful Python-like shortcut is that plain ``K`` is allowed as a draft form: the checker warns and can autofix it to the inferred variance.

Lucid also makes mutability part of the type spelling without making common mutable code verbose. ``InferenceModel[K]`` is the ordinary mutable type. ``InferenceModel?[K]`` is the read-only view. ``InferenceModel![K]`` is the immutable type. This mirrors Scala's virtue of making collection and API mutability visible in the type, while adapting the surface syntax to Python-like code where mutable objects are common.

Class-level behavior
--------------------

Python represents class methods and static methods through decorators on ordinary functions.

Lucid has syntax for class-level behavior:

* ``classmethod`` is inherited behavior on the class object
* ``factory`` is a non-inherited constructor for the exact defining class
* helper functions that do not use ``self`` or ``cls`` remain ordinary functions

Properties and descriptors
--------------------------

Python descriptors, ``property``, ``__getattribute__``, ``__getattr__``, ``__setattr__``, and ``__delattr__`` can make attribute access programmable from many places.

Lucid replaces that with explicit member kinds: fields, methods, class methods, factories, getters, setters, and class member variables.

Composition
-----------

Python uses one inheritance system for concrete inheritance, abstract base classes, mixins, and method-resolution-order composition.

Lucid separates those roles:

* classes own data and concrete behavior
* interfaces declare required APIs
* traits provide reusable method bodies
* trait conflicts are resolved explicitly by the class

Lucid keeps Python's useful abstract-method instantiation check, but makes the marker part of the language. A remaining ``declare`` member makes a class abstract for construction purposes, so there is no separate ``@abstractmethod`` decorator.

Truth
-----

Python truthiness falls back through ``__bool__``, ``__len__``, and built-in emptiness rules. Lucid requires boolean control flow to use ``bool`` or an explicit ``__bool__``.

Collection literals
-------------------

Python uses ``{}`` for an empty dictionary and requires ``set()`` for an empty set. Lucid uses ``{}`` for an empty set and ``{:}`` for an empty dictionary.

Lazy imports
------------

Python already has lazy-import building blocks, such as import hooks and lazy loaders. Lucid makes laziness explicit in the source with ``lazy import`` and ``lazy from`` forms, so a Python backend can lower them to those mechanisms or to a generated proxy binding.

Indexing
--------

Python parses ``x[1, 2, 3]`` as a single tuple index. Lucid treats comma-separated indexing as multiple index arguments. Use ``x[(1, 2, 3)]`` when the intended index is a tuple.

Operators
---------

Python binary operators are left-owned first, then may try reflected methods on the right operand. Lucid treats binary operators as multiple-dispatch functions over both operands.

Removed object-model hooks
--------------------------

Lucid removes object-model features that can rewrite class creation, type relationships, object identity, or cleanup from indirect locations.

.. list-table::
   :header-rows: 1

   * - Python feature
     - Lucid approach
   * - metaclasses
     - not part of the language
   * - ``__prepare__``
     - metaclass machinery absent
   * - ``__mro_entries__``
     - no MRO rewriting
   * - ``__instancecheck__``, ``__subclasscheck__``
     - no programmable type checks
   * - assigning ``obj.__class__``
     - object shape is fixed
   * - adding/removing class members after definition
     - class shape is closed
   * - ``__del__``
     - cleanup must be explicit
   * - implicit instance ``__dict__``
     - use explicit dictionary fields

A restricted ``__init_subclass__(cls, **options)`` may remain for validation and registration only, not for class rewriting.
