Problems with Python that Lucid fixes
=====================================

Lucid borrows Python's readable syntax, but changes the parts of Python where behavior can be hidden behind dynamic object-model hooks, scattered conventions, or checker-specific side channels.

Object shape
------------

Python instances usually have an open-ended ``__dict__``, unless a class uses ``__slots__``, dataclasses with options, extension types, or custom attribute hooks. Lucid makes fixed shape the default: stored fields are declared in the class body, and undeclared fields are rejected.

Use an explicit dictionary field when dynamic keys are part of the model.

Discarded values
----------------

Python treats ``_`` as an ordinary name by default, even though many codebases
use it by convention for ignored values. Lucid makes ``_`` a black-hole keyword:
assigning to it discards the value, and using it in an expression is invalid.

Lucid also adds ``skip`` for conditional elision inside collection literals and
call arguments. ``skip`` is not a value and cannot be returned or assigned. In a
list, tuple, or set literal, an entry that reaches ``skip`` is omitted:

.. code-block:: python

   [1, 2, 3 if False else skip, 4] == [1, 2, 4]

In a dictionary literal, an entry is omitted if either the key or the value
reaches ``skip``:

.. code-block:: python

   {1: 2, 3: skip, skip: 6} == {1: 2}

In a call, positional and keyword arguments that reach ``skip`` are omitted:

.. code-block:: python

   print(1, skip, 3, file=skip)

means ``print(1, 3)``.

Scope rebinding
---------------

Python uses ``global`` and ``nonlocal`` declarations to make assignment inside a
function rebind a name from a module or enclosing function scope:

.. code-block:: python

   counter = 0

   def next_id() -> int:
       global counter
       counter += 1
       return counter

and:

.. code-block:: python

   def make_counter():
       count = 0

       def next():
           nonlocal count
           count += 1
           return count

       return next

Lucid removes both declarations. Reading outer bindings is allowed, but
assignment to a name is local to the current function. Code that needs shared
state uses an explicit mutable object:

.. code-block:: python

   counter = Cell(0)

   def next_id() -> int:
       counter.value += 1
       return counter.value

or puts the state on the object that owns it:

.. code-block:: python

   class IdSource:
       next: int = 0

       def take(self) -> int:
           value = self.next
           self.next += 1
           return value

Closure state follows the same rule:

.. code-block:: python

   def make_counter() -> Callable[[], int]:
       count = Cell(0)

       def next() -> int:
           count.value += 1
           return count.value

       return next

This keeps assignment direct: a name assignment binds locally, while mutation of
shared state is visible as member or item mutation on an explicit object.

Construction
------------

Python splits construction across ``__new__``, ``__init__``, dataclass-generated initializers, ``__post_init__``, ``InitVar``, and field options such as ``init=False`` or ``kw_only``.

Lucid has one construction model: factories return fully constructed objects through the factory-only ``construct`` keyword.

Every class also gets a generated ``replace`` factory for copy-with-update construction. Because it is a factory, it follows the same exact-class rule as other construction forms and is not inherited.

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

Python descriptors, ``property``, ``__getattribute__``, ``__getattr__``,
``__setattr__``, and ``__delattr__`` can make attribute access programmable
from many places.

Lucid replaces that with explicit member kinds: fields, methods, class methods, factories, getters, setters, and class member variables.

Lucid does not include ``__getattr__`` or ``__getattribute__`` fallback hooks.
An attribute read is valid when the member is visible in the class body as a
field, method, getter, class method, factory, or class member variable. Missing
attributes are errors instead of calls into dynamic lookup code.

Lucid also does not include ``__setattr__``. Attribute assignment targets a
declared field or an explicit setter. Dynamic per-object keys belong in an
ordinary dictionary field:

.. code-block:: python

   class Record:
       fields: dict[str, object]

       def get(self, name: str) -> object | None:
           return self.fields.get(name)

       def set(self, name: str, value: object):
           self.fields[name] = value

Lucid does not include ``__del__`` finalizers. Cleanup should be explicit in the
API that owns the resource, instead of being hidden behind object destruction
timing.

Composition
-----------

Python uses one inheritance system for concrete inheritance, abstract base classes, mixins, and method-resolution-order composition.

Lucid separates those roles:

* classes own data and concrete behavior
* interfaces declare required APIs
* traits provide reusable method bodies
* trait conflicts are resolved explicitly by the class

Lucid keeps Python's useful abstract-method instantiation check, but makes the marker part of the language. A remaining ``declare`` member makes a class abstract for construction purposes, so there is no separate ``@abstractmethod`` decorator.

Truth and Boolean values
------------------------

Python truthiness falls back through ``__bool__``, ``__len__``, and built-in emptiness rules. Lucid requires boolean control flow to use ``bool`` or an explicit ``__bool__``.

Python also makes ``bool`` a subclass of ``int``. That means ``int`` annotations can accept boolean values and arithmetic or bitwise operators can silently treat flags as numbers.

Lucid keeps ``bool`` separate from ``int`` and removes the numeric tower
entirely. Numeric capability is expressed through structural interfaces such as
``SupportsInt``, ``SupportsFloat``, ``SupportsComplex``, and ``SupportsIndex``,
plus operation-specific interfaces such as ``SupportsAbs`` and
``SupportsRound``, and broader operation bundles such as ``IntLike``,
``FloatLike``, and ``ComplexLike``, not through abstract numeric base classes.
These interfaces are builtins. Boolean values do not support numeric arithmetic
or bitwise arithmetic operators such as ``+``, ``-``, ``*``, ``/``, ``%``,
``|``, ``&``, and ``^``. An ``int`` annotation means ``int``; use ``int | bool``
only when both types are intended.

Lucid also keeps ``float`` and ``complex`` annotations exact. ``float`` does not
mean ``int | float``, and ``complex`` does not mean ``int | float | complex``.
Use a union or a ``Supports...`` interface when a broader capability is
intended.

``SupportsIndex`` remains separate from ``SupportsInt`` because exact
indexability is not the same as explicit integer conversion. ``int(x)`` is an
explicit conversion and may be lossy for some types; it does not make ``x`` an
``int`` for annotation or indexing purposes.

Numeric equality and ordering are type-directed. Cross-type numeric equality,
cross-type hashing, and cross-type ordering exist only where explicitly defined.
``bool`` and ``complex`` are not orderable. Bitwise operators are integer-like
operations, not general numeric operations, and are not provided by ``bool``,
``float``, or ``complex``.

Because Lucid binary operators are multiple-dispatch functions, an interface
cannot promise an operator by declaring a left-owned method. It uses ``declare
dispatch`` instead, which says that a matching implementation of that generic
operation must exist for the specified operand types. Checking that promise is
global to the generic operation's dispatch table: if an interface requires
``__add__(Self, C)``, then concrete type ``A`` satisfies it when some applicable
``__add__`` dispatch exists for ``(A, C)``. Applicability follows Julia's method
model: a method written for parent classes or interfaces of ``A`` or ``C`` also
satisfies the obligation. The implementation may be written with ``A``, with
``C``, or in another permitted extension location.

This is close to Julia's model: operators such as ``+`` are generic functions,
and typed definitions add methods to those functions. Julia's interfaces are
informal collections of methods a type should implement. Lucid makes the same
kind of obligation explicit in interfaces.

This matches the way numerical libraries usually treat Boolean arrays as logical values rather than ordinary integers. It also removes footguns such as ``True + True == 2`` and ``True & 4 == 0``, where the result differs from logical control-flow intuition. These operations are uncommon, and the old behavior remains explicit as ``int(flag)``. Keeping the ``bool`` interface small also helps static checkers catch accidental leaks of Boolean flags into arithmetic code.

Collection literals
-------------------

Python uses ``{}`` for an empty dictionary and requires ``set()`` for an empty set. Lucid uses ``{}`` for an empty set and ``{:}`` for an empty dictionary.

Lucid also uses the immutable marker ``!`` on collection literals. ``!{a, b}``
constructs a frozenset, and ``!{a: b}`` constructs a frozendict. The empty forms
follow the same distinction: ``!{}`` is an empty frozenset, and ``!{:}`` is an
empty frozendict.

String literals
---------------

Python concatenates adjacent string literals at compile time. Lucid rejects adjacent string literals. Use an explicit concatenation operation when a string is meant to be joined.

Python also treats ``str`` as ``Sequence[str]`` for static typing. Lucid keeps
``str`` narrower: it is ``Container[str]`` and ``Sized``, but not
``Iterable[str]``, ``Collection[str]``, or ``Sequence[str]``. Strings do not
provide ``__iter__``. Use ``text.chars()`` when code intentionally wants a
``Sequence[str]`` view of the string's characters.

This avoids accidental character-by-character use of strings in APIs that ask
for general sequences or iterables. The cost is explicit at the call site:
write ``chars()`` when character traversal is intended, or use a cast when
bridging to code that deliberately accepts strings as sequences.

Lazy imports
------------

Python already has lazy-import building blocks, such as import hooks and lazy loaders. Lucid makes laziness explicit in the source with ``lazy import`` and ``lazy from`` forms, so a Python backend can lower them to those mechanisms or to a generated proxy binding.

Indexing
--------

Python parses ``x[1, 2, 3]`` as a single tuple index. Lucid treats comma-separated indexing as multiple index arguments. Use ``x[(1, 2, 3)]`` when the intended index is a tuple.

Python also has an old sequence-iteration fallback: if an object has
``__getitem__`` but no ``__iter__``, iteration may call ``__getitem__`` with
successive integers until indexing fails. Lucid removes that sequence protocol.
Indexing and iteration are separate capabilities. A type is iterable only if it
implements or inherits from ``Iterable``.

Call arguments
--------------

Python treats ``f(x for x in items)`` as a call with one generator object
argument. Lucid treats a generator expression written directly as a call
argument as argument expansion. For example, ``f(x for x in [x_1, x_2, x_3])``
means ``f(x_1, x_2, x_3)``.

Loop search clauses
-------------------

Python loop ``else`` clauses run when a loop finishes without ``break``. Lucid
removes loop ``else`` and adds ``if_broken`` for the opposite case.

Lucid adds an optional ``if_broken`` clause for loops. The loop body can focus on
searching, ``if_broken`` handles the found-by-break case, and ordinary
fall-through code handles the exhausted-without-break case:

.. code-block:: python

   def find_match(items: Iterable[Item]) -> Item | None:
       for item in items:
           if is_match(item):
               found = item
               break
       if_broken:
           return found
       return None

The keyword is ``if_broken``, a single soft keyword in loop syntax. This avoids
the misleading ``if break`` spelling, which would give ``if`` and ``break`` a
combined meaning they do not have elsewhere. The cost is that parsers,
highlighters, and other code-aware tools need to learn the new soft keyword.

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
   * - ``__getattr__``, ``__getattribute__``
     - attribute reads use visible members
   * - ``__setattr__``
     - attribute assignment uses declared fields or setters
   * - implicit instance ``__dict__``
     - use explicit dictionary fields

A restricted ``__init_subclass__(cls, **options)`` may remain for validation and registration only, not for class rewriting.
