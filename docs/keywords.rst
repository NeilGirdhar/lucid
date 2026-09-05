Keyword reference
====================

.. contents:: Table of contents
   :depth: 2
   :local:

Lowercase constants
-----------------------

Lucid spells its boolean and null constants in lowercase:

.. code-block:: text

   false none true

Preserved Python keywords
-----------------------------

Apart from the lowercase constants and behaviors explicitly replaced in this
specification, Lucid preserves Python's ordinary keyword vocabulary. The
preserved Python keywords are:

.. code-block:: text

   and as assert async await break class continue def del elif else except
   finally for from if import in is not or pass raise return try while
   with yield

New Lucid keywords
-----------------------

Lucid adds keywords for explicit module boundaries, construction,
call-site capture, class member kinds, closing off rebinding or further
class inheritance or overriding, explicit overrides, declining generated
behavior, abstraction, dispatch, external interface implementation, type
expressions, existential quantification, exhaustive pattern matching, and
elision:

.. code-block:: text

   export
   classmethod classvar factory construct
   caller from_var_name
   getter setter
   final sealed override
   without
   interface trait
   dispatch
   implement
   type
   any
   trust
   match case
   if_broken
   skip
   _

Discarded Python keywords
-----------------------------

Lucid discards Python's scope-rebinding declarations — see the ``No global``
and ``No nonlocal`` rules in
`Names, binding, and scope <names.rst>`_:

.. code-block:: text

   global nonlocal
   lambda

``else`` is not discarded as a keyword. Lucid removes loop ``else`` clauses,
but ``else`` remains available for the Python-like constructs that still use
it. ``lambda`` is discarded because it is redundant, not because anonymous
functions are gone: an unnamed ``def`` is one (see
`Anonymous functions <calls.rst>`_), needing no keyword of its own.
