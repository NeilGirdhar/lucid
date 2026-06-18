Lucid documentation
===================

Lucid is a Python-like language sketch that keeps Python easy to read and write
while making room for cleaner type, dispatch, object, and compatibility rules.

Core principle:

    Keep Python's directness, make structure explicit, and choose the cleaner
    rule when compatibility no longer has to win.

Lucid borrows Python's readable surface syntax, Scala-style definition-site type
information, Julia-style multiple dispatch, dataclasses' transparent field-first
objects, and the freedom to follow Python Enhancement Proposals that Python
could not adopt because of backward compatibility.

Reading path
------------

Start with the README for the short version, then read the language spec
front-to-back.

.. list-table::
   :header-rows: 1

   * - Document
     - Purpose
   * - `The Lucid language <language.rst>`_
     - The readable specification of the current language sketch, with rationale
       for each difference from Python.

The language document is the source of truth for Lucid semantics.

Commitments
-----------

Lucid's main commitments are:

* Python-like readability and writeability
* Scala-style visible type relationships
* Julia-style multiple dispatch
* dataclass-like transparent object state
* freedom to choose PEP-inspired improvements without Python compatibility limits
* stored object state is declared in the class body
* construction returns fully built objects
* public module APIs are marked with ``export``
* interfaces declare required behavior
* traits provide reusable method bodies
* binary operators dispatch on both operands
* dynamic object-model hooks are not part of the language

Current slogan
--------------

    Classes store data. Interfaces specify obligations. Traits provide reusable behavior. Factories construct exact classes. Attribute access is structural, not magical. Exports define the public API.
