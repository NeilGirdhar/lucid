1. Introduction
===============

Lucid is a Python-like language sketch built around explicit structure, exact construction, and a smaller object model.

Core principle:

    Make structure explicit, make construction exact, and remove hidden object-model magic.

Lucid keeps Python's readable surface syntax, but makes object shape, public APIs, type relationships, and construction rules visible in the program text.

1.1. Design commitments
-----------------------

Lucid's main commitments are:

* stored object state is declared in the class body
* construction returns fully built objects
* public module APIs are marked with ``export``
* interfaces declare required behavior
* traits provide reusable method bodies
* binary operators dispatch on both operands
* dynamic object-model hooks are not part of the language

1.2. Notation
-------------

This sketch uses Python-like examples for source code and plain text diagrams for type relationships. It is not yet a full grammar.
