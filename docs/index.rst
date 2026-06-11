Lucid language sketch
=====================

Lucid is a Python-like language sketch built around explicit structure, exact construction, and a smaller object model.

Core principle:

    Make structure explicit, make construction exact, and remove hidden object-model magic.

Lucid keeps Python's readable surface syntax, but makes object shape, public APIs, type relationships, and construction rules visible in the program text.

Documentation
-------------

* `Language reference <reference/index.rst>`_
* `Problems with Python that Lucid fixes <python-fixes.rst>`_

The language reference is organized after Python's own reference: lexical structure, data model, execution model, imports, expressions, statements, top-level forms, and grammar. The Python-fixes document is deliberately separate. It explains why Lucid changes particular Python behaviors without mixing design rationale into the normative reference.

Commitments
-----------

Lucid's main commitments are:

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
