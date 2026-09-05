Mutability
==========

.. contents:: Table of contents
   :depth: 3
   :local:

Lucid makes mutability part of the type spelling. Mutable and immutable
variants of the same abstraction are declared as one type family, spelled
with a marker on the short, unqualified name:

- ``T`` — mutable, the default. Code can read and write it.
- ``&T`` — read-only view. Code can observe it but cannot mutate it, and
  cannot rely on it being permanently immutable.
- ``!T`` — immutable. Code can rely on stability for operations such as
  hashing, memoization, and persistent sharing.

.. code-block:: python

   working: InferenceModel[str] = InferenceModel(weights, metadata, {:})
   stable: !InferenceModel[str] = freeze(working)
   view: &InferenceModel[str] = working

Python's ``collections.abc`` needs a separate, hand-written class for each
capability level: ``Mapping`` declares the read-only methods, and
``MutableMapping`` is a second class, written by hand, adding the mutating
ones. Nothing stops a third, hand-written ``ImmutableMapping`` from joining
them — Python just never added one, and ``Mapping`` would be a poor
foundation for it anyway: it doesn't promise "no mutation," only that this
one reference hides mutating methods. The underlying object keeps whatever
methods its real class has, and anyone holding another reference can still
call them:

.. code-block:: python

   from collections.abc import Mapping

   underlying = {"a": 1}
   view: Mapping[str, int] = underlying
   underlying["a"] = 2
   view["a"]  # 2 -- "read-only", but not stable

Lucid generates the family instead of asking an author to hand-write it.
Declare a type once, with its ordinary mutable methods, and ``&T`` and
``!T`` both follow from that single declaration: ``&T`` is the same
interface with every mutating operation removed, and ``!T``, produced by
``freeze``, is the runtime-frozen instance of it. Both are subtypes of
``&T`` because both satisfy its reduced interface — the mutable original
by simply having more methods than it needs to, the frozen form by
actually being what ``&T`` only promised to look like:

.. code-block:: text

   InferenceModel[K]  <: &InferenceModel[K]
   !InferenceModel[K] <: &InferenceModel[K]

Read-only view
----------------

As a function parameter
~~~~~~~~~~~~~~~~~~~~~~~~~~

Without a distinct read-only view, a parameter type is stuck between two bad
options. Make it invariant, and a function that only reads ``Animal``\ s
can't accept a ``list[Cat]`` argument even though reading is always safe.
Make it covariant instead, and nothing stops the function from writing a
``Dog`` into what is actually the caller's ``list[Cat]``, corrupting it.
``&T`` escapes that dilemma: it is the natural type for a parameter that
only reads its argument, and because both ``T`` and ``!T`` are subtypes of
``&T``, a single ``&T``-typed parameter accepts a mutable value, an
immutable value, or another read-only view, with no conversion at the call
site — while the callee gets a compile-time guarantee that it cannot mutate
an object it does not own:

.. code-block:: python

   def report(model: &InferenceModel[str]) -> str:
       return f"{model.label_count} labels"

   report(working)  # mutable
   report(stable)   # immutable
   report(view)      # already a read-only view

Safe covariance
~~~~~~~~~~~~~~~~~~

Variance is computed separately for each view. Mutable types are usually
invariant because they both produce and consume their type parameters, but
read-only and immutable views can often be covariant — this is exactly the
covariance the parameter dilemma above needed, made sound because the view
itself blocks writes. The same pattern applies to any type family with these
three views: the mutable variant is typically invariant, while the
read-only and immutable views can each be declared with the narrowest
variance their own operations support:

.. code-block:: text

   InferenceModel[=K]
   &InferenceModel[+K]
   !InferenceModel[+K]

Read-only dictionaries
~~~~~~~~~~~~~~~~~~~~~~~~

This avoids the old split between mutable dictionaries and read-only mapping
interfaces. A mutable ``dict[str, Cat]`` should not be usable as a
``dict[str, Animal]`` because the receiver could write a ``Dog`` into it. But a
read-only view can safely widen the produced value type:

.. code-block:: python

   cats: dict[str, Cat] = {:}
   animals: &dict[str, Animal] = cats

   animal = animals["ada"]
   animals["turing"] = Dog()  # error: read-only view

Python's ``Mapping`` does not fully solve this. It is a separate abstraction
from ``dict``, and its key parameter is still invariant because the mapping API
both accepts keys for lookup and produces keys through views such as
``keys()``. Library authors still have to choose a different name and API
surface to ask for read-only dictionary access, and they only get the variance
that ``Mapping`` happened to declare.

Lucid keeps these as views of the same collection abstraction and computes
variance from each view's actual operations. Mutable ``dict`` stays invariant.
A full read-only dictionary view such as ``&dict[K, +V]`` is covariant in the
value type while keeping the key type invariant if the view both consumes and
produces keys. A narrower view that only produces keys or values can expose
different variance. Code does not need a separate ``Mapping`` type just to ask
for a read-only dictionary-shaped view.

Immutable view
----------------

``!T`` is for code that needs to rely on stability, not just observe a
snapshot of it. An immutable value can be hashed and used as a dict key or
set member, memoized safely since a cached result can never go stale, and
shared freely across threads, caches, and closures without defensive
copying — nothing holding a ``!T`` can ever see it change underneath it.

.. code-block:: python

   cache: dict[!InferenceModel[str], float] = {:}
   cache[stable] = evaluate(stable)

A mutable value becomes a ``!T`` through ``freeze``, which takes a ``T`` and
returns the immutable view.

.. code-block:: python

   stable: !InferenceModel[str] = freeze(working)

