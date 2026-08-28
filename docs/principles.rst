Main ideas
==========

Lucid is a Python-like language sketch that keeps Python easy to read and
write while removing the compatibility constraints that keep Python from
adopting many of its own best proposals.

Core principle:

    Keep Python's directness, make structure explicit, and choose the cleaner
    rule when compatibility no longer has to win.

.. contents:: Table of contents
   :depth: 2
   :local:

Python readability
-------------------

Lucid code should stay easy to read and write in the same way ordinary Python
is easy to read and write: indentation matters, definitions are direct,
common control flow is familiar, and simple programs do not need ceremony.
See `Source basics and names <source-and-names.rst>`_ and
`Control flow and statements <control-flow.rst>`_.

Scala-style type information
------------------------------

Type relationships live where abstractions are defined. Generic parameters
carry definition-site variance with ``+K``, ``-K``, and ``=K``. Mutable,
read-only, and immutable views are visible in the type spelling with ``T``,
``T?``, and ``T!``. See `Types, mutability, and annotations <types.rst>`_ and
`Modern type specification <type-specification.rst>`_.

Julia-style dynamic dispatch
------------------------------

Operations can dispatch on all relevant runtime argument types. Binary
operators are generic multiple-dispatch operations: they are not owned by the
left operand, and Lucid does not use reflected methods or ``NotImplemented``
as an operator negotiation protocol. See
`Calls, indexing, and operators <operators.rst>`_.

Freedom to follow better PEPs
-------------------------------

Python often cannot adopt cleaner rules because existing programs depend on
the old behavior. Lucid treats those rejected or constrained improvements as
design space: when a PEP points toward a clearer language but compatibility
blocks it, Lucid can choose the clearer rule. Most of the differences from
Python documented throughout the rest of this specification trace back to
this freedom.

Keyword reference
-------------------

Lowercase constants
~~~~~~~~~~~~~~~~~~~~

Lucid spells its boolean and null constants in lowercase:

.. code-block:: text

   false none true

Preserved Python keywords
~~~~~~~~~~~~~~~~~~~~~~~~~~

Apart from the lowercase constants and behaviors explicitly replaced in this
specification, Lucid preserves Python's ordinary keyword vocabulary. The
preserved Python keywords are:

.. code-block:: text

   and as assert async await break class continue def del elif else except
   finally for from if import in is lambda not or pass raise return try while
   with yield

New Lucid keywords
~~~~~~~~~~~~~~~~~~~

Lucid adds keywords for explicit module boundaries, construction, class
member kinds, abstraction, dispatch, type expressions, and elision:

.. code-block:: text

   export
   classmethod classvar factory construct
   getter setter
   interface trait declare
   dispatch
   type
   if_broken
   skip
   _

Discarded Python keywords
~~~~~~~~~~~~~~~~~~~~~~~~~~

Lucid discards Python's scope-rebinding declarations — see the ``No global``
and ``No nonlocal`` rules in
`Source basics and names <source-and-names.rst>`__:

.. code-block:: text

   global nonlocal

``else`` is not discarded as a keyword. Lucid removes loop ``else`` clauses,
but ``else`` remains available for the Python-like constructs that still use
it.
