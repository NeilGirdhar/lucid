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

Mutable types are usually invariant, because they both produce and
consume their type parameters — but a read-only or immutable view drops
every mutating member, and often loses whichever use forced invariance
in the first place. That is exactly the covariance the parameter dilemma
above needed, made sound because the view itself blocks writes. One
marker on the mutable declaration settles the variance of all three
views; see `Variance under &T and !T <generics.rst>`_ for the full rule
and why it never needs more than one:

.. code-block:: text

   InferenceModel[+=K]

which reads as: invariant while mutable — ``score`` writes to
``self._scores``, consuming ``K`` — but covariant once read-only or
immutable, since the write that forced invariance is gone and only
``labels: list[K]``'s read remains.

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

Lucid keeps these as views of the same collection abstraction instead,
with one declaration governing all three:

.. code-block:: text

   dict[=K, +=V]

``K`` stays invariant everywhere: both of its uses — ``get``'s lookup and
``keys()``'s enumeration — are non-mutating, so both survive onto the
read-only view unchanged, and invariance survives with them. ``V`` is
only ever produced by a non-mutating member (``get``) and only ever
consumed by a mutating one (``__setitem__``), so the read-only view
drops the one member forcing invariance and loosens to ``+V``. A
narrower view that exposes only keys or only values can land on
different variance again, for the same reason. Code does not need a
separate ``Mapping`` type just to ask for a read-only dictionary-shaped
view.

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

A mutable value becomes a ``!T`` through ``freeze``, which takes a ``T``
and returns an immutable copy — not a view onto the same storage, since a
view would leak mutations from whoever still holds the original ``T``, the
same flaw that makes Python's ``Mapping`` unsound.

.. code-block:: python

   stable: !InferenceModel[str] = freeze(working)

Freezing is deep
~~~~~~~~~~~~~~~~~~~~

``freeze`` freezes every field, recursively, bottoming out at scalars and
other values with no mutable state to begin with. Nothing shallower would
honor the guarantee this section opened with: "nothing holding a ``!T``
can ever see it change underneath it" is false the moment one field is
still a plain ``T`` that some other part of the program can still reach
and mutate. A shallow freeze would just be a second name for ``&T``,
which already covers "this one reference can't write to it."

Sharing, reuse, and cycles
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``freeze`` behaves as though it always copies, but two things follow from
what a recursive, sharing-aware copy actually has to do.

First, sharing survives. If two fields hold the same mutable sub-object
before freezing, they hold the same frozen sub-object after — one frozen
value, still referenced twice, not two independent copies. Losing that
would silently break any code that compares frozen values by identity,
and would duplicate storage for structure the original never duplicated.

Second, copying itself is only ever an implementation of the "nothing can
see it change underneath it" guarantee, not the guarantee itself. Once
nothing else in the program still holds a live mutable alias to a piece
of state, freezing it needs no copy — reusing the storage in place is
unobservable, since the only thing that could have exposed the reuse is
gone. `Freezing is deep`_ describes what every frozen value must behave
as if true; reuse is the implementation taking that "as if" literally
whenever it costs nothing to.

Neither of these has an answer for a cycle. A mutable structure with a
back-reference — a child pointing back to its parent, say — has no
scalar to bottom out at: freezing the parent needs the child already
frozen, and freezing the child needs the parent already frozen. Rather
than returning a value that is only partly frozen, or looping forever,
``freeze`` raises when the graph it is given is cyclic. A frozen value
that is not fully, honestly immutable is worse than no frozen value at
all.

Equality, ordering, and hashing
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Lucid generates three things for every class by default, the same way it
generates a default constructor when ``__init__`` is left unspecified
(see `Classes <classes.rst>`_): structural ``Eq``, structural ``Ord``,
and hashability. All three are traits, not interfaces — each provides a
real body, not just an obligation, because each just iterates over the
class's own fields and does the obvious thing: ``Eq`` compares them all,
``Ord`` compares them in field-declaration order, ``Hashable`` combines
their hashes.

`Field reflection with fields <construction.rst>`_ yields a class's own fields
in declaration order — the same walk the generated constructor and
``replace`` already do — which is what lets each trait below be one real,
shared body instead of something synthesized fresh per class:

.. code-block:: python

   trait Eq:
       def dispatch __eq__(lhs: Self, rhs: Self) -> bool:
           return all(l.value == r.value for l, r in zip(fields(lhs), fields(rhs)))

   trait Ord(Eq):
       def dispatch __lt__(lhs: Self, rhs: Self) -> bool:
           ...
       def dispatch __le__(lhs: Self, rhs: Self) -> bool:
           ...
       def dispatch __gt__(lhs: Self, rhs: Self) -> bool:
           ...
       def dispatch __ge__(lhs: Self, rhs: Self) -> bool:
           ...

   trait Hashable:
       def __hash__(self: !Self) -> int:
           ...

``Ord``'s ``__lt__`` does the real field-by-field work; ``__le__``,
``__gt__``, and ``__ge__`` are each defined generically in terms of
``__lt__`` and ``__eq__`` instead of walking fields a second time
(``a <= b`` is ``a < b or a == b``, ``a > b`` is ``b < a``, and so on). A
class that replaces ``__lt__`` gets correct ``__le__``/``__gt__``/``__ge__``
for free, unless it replaces those too.

A class declines any of the three with ``without``, required before a
comparison operator can return anything but ``bool`` — the same way
`Explicit overrides <traits.rst>`_ requires ``override`` before a trait
method's body can be replaced, so the departure is marked, not silent:

.. code-block:: python

   class complex without Ord:
       ...

   class Array without Eq, Ord:
       def dispatch __eq__(lhs: Array, rhs: Array) -> Array:
           ...
       def dispatch __lt__(lhs: Array, rhs: Array) -> Array:
           ...

``complex`` declines only ``Ord`` — two complex values have no
less-than to compare (see `Numeric types <numeric-types.rst>`_) — and
keeps ``Eq`` and ``Hashable``. ``Array`` declines both: ``==`` and ``<``
compare elementwise, returning another ``Array`` of booleans rather than
one ``bool``, because two arrays that agree in some positions and
disagree in others have no single answer to give. Declining ``Eq`` forces
declining ``Ord`` and ``Hashable`` along with it — an ordering or a hash
both presuppose the equality they must stay consistent with — while
``Ord`` and ``Hashable`` can each be declined on their own, independently
of the other.

``T`` and ``&T`` are never hashable no matter what ``Eq``/``Ord``/
``Hashable`` say, for the reason `Read-only view`_ already gave for
mutation: ``T``'s storage can still change, and ``&T`` views an object
that can still change through some other reference. Only ``!T`` can
actually call ``__hash__``, which is why ``Hashable`` requires
``self: !Self`` rather than the ordinary default. Because freezing is
deep, every field of a hashable ``!T`` is itself hashable, so Lucid
derives ``__hash__`` automatically from those fields — the same way it
derives the rest of ``!T``'s interface from one declaration rather than
asking an author to hand-write it.

``dict`` and ``set`` require ``!Hashable`` keys
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

A key or set member has to survive being hashed once and looked up again
later, so both bound their element parameter to ``!Hashable``:

.. code-block:: text

   dict[K: !Hashable, +V]
   set[K: !Hashable]

``!Hashable`` reads the same way ``!InferenceModel`` does — the immutable
view, here of anything satisfying ``Hashable`` rather than of one named
class. ``K: Hashable`` alone would not be enough: that only asks whether
a class has declined ``Hashable``, a fact independent of which view is in
hand, and a mutable ``T`` can satisfy it in name while still being
impossible to actually hash. ``!Hashable`` asks for both at once:

.. code-block:: python

   cache: dict[InferenceModel[str], float] = {:}   # error: not !Hashable
   cache: dict[!InferenceModel[str], float] = {:}  # fine
   index: dict[!Array, float] = {:}                # error: Array declined Hashable

Scalars such as ``str`` and ``int`` are ``!Hashable`` with no separate
frozen form to reach for, since they have no mutating methods to
distinguish ``T`` from ``!T`` in the first place — the same reason
``&float`` was already "mainly useful for uniform view syntax" rather
than a real second form (see `Numeric types <numeric-types.rst>`__).

Containers supply their own bodies
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``list``, ``dict``, and ``set`` satisfy ``Eq`` — and, where it makes
sense, ``Ord`` and ``Hashable`` — the same way any class can: by
supplying their own body instead of the one ``fields()`` would generate.
None of the three own named fields to walk; they own elements, so the
generic body would not even type-check for them, the same reason it
would not for ``Array``. Unlike ``Array``, their comparisons still return
``bool``, so none of them need ``without``.

Order matters for a ``list``: its equality and hash walk elements by
position, so ``[1, 2] != [2, 1]``. A ``dict`` and a ``set`` have no
positions — two dicts with the same pairs inserted in a different order
are equal, and a ``set`` equals any reordering of itself — so their
``__eq__`` and ``__hash__`` combine elements order-independently instead.
None of the three decline ``Hashable`` for any of this: a frozen
``list``/``dict``/``set`` is hashable exactly when its elements are, the
same rule as everywhere else.

``dict`` declines ``Ord`` outright — two dicts with different keys have
no natural less-than, the same reason ``complex`` declines it (see
`Numeric types <numeric-types.rst>`__). ``set`` keeps ``Ord``, but gives
``<`` a different meaning than lexicographic order: proper subset,
matching Python. Only ``__lt__`` and ``__eq__`` need a body of their
own — ``__le__``, ``__gt__``, and ``__ge__`` are already defined
generically in terms of those two, and "proper subset or equal" already
means exactly "subset," so the generic ``__le__`` is correct for ``set``
unchanged.

Default values
------------------

A default value — for a field or for an ordinary function or method
parameter — is evaluated according to its own declared type, the same
``T``/``!T`` distinction this document opened with. A ``!T`` default is
evaluated once and shared: nothing can mutate it, so sharing is harmless.
A ``T`` default is evaluated fresh at every call or construction instead:

.. code-block:: python

   class Config:
       tags: list[str] = []        # a fresh, empty list every time
       limits: !list[int] = ![]    # evaluated once, shared safely

   def process(seen: list[str] = []):  # also fresh every call
       ...

Python evaluates every default once, at definition time, and shares the
result across every call afterward. For an immutable default this is
invisible — sharing a value nothing can change is indistinguishable from
recomputing it — but for a mutable one it is the language's most
notorious footgun: ``def f(x=[]): x.append(1)`` accumulates across calls
that never intended to share anything. Python's own fix,
``dataclasses.field(default_factory=list)``, covers only dataclass
fields, leaves ordinary function parameters exposed, and asks an author
to remember which of two spellings a given default needs. Lucid needs
neither spelling nor memory: the type already says whether sharing is
safe, so evaluation timing follows from it automatically, for a field
default and a parameter default alike.

An impure default — one computed for a side effect rather than a value,
such as a logging call — is not fully solved by this rule: its type may
be trivially immutable while the side effect itself still only wants to
run once, or once per call, an intent neither ``T`` nor ``!T`` records.
This is a narrower, rarer problem than the aliasing one above, and Python
does not solve it either.

A default cannot reference another parameter of the same signature —
``def g(a: int, b: int = a + 1):`` is an error, not a later-bound
expression evaluated once ``a`` is known. Python already forbids this,
if only by accident: its defaults evaluate once, in the enclosing scope,
before any parameter exists to reference. Lucid's own per-call
evaluation for ``T`` defaults would make it technically possible —
``a`` really is in scope by the time ``b``'s default would run — but
allowing it anyway would turn an independent, per-parameter rule into a
dependency chain sensitive to parameter order: reordering ``a`` and
``b``, or giving ``a`` its own default, would silently change what
``b``'s default means. A default stays exactly what it already is
elsewhere in this section — an expression evaluated on its own, once or
per call depending on its type — never one that reads another parameter
to compute itself.

