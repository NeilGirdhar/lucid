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

Start with the README for the short version, then read the specification
documents front-to-back.

.. list-table::
   :header-rows: 1

   * - Document
     - Purpose
   * - `The Lucid language <language.rst>`_
     - The readable specification of the current core language sketch, with
       rationale for each difference from Python.
   * - `Modern type specification <type-specification.rst>`_
     - The interface, trait, and class model for user-defined types.
   * - `Project configuration <project-configuration.rst>`_
     - The structure of ``project.yaml`` and ``development.yaml``, and how
       they replace ``pyproject.toml`` and ``__init__.py``.

The language, type specification, and project configuration documents are the
source of truth for Lucid semantics.

Main ideas
----------

The language document opens with the main ideas behind Lucid's syntax, values,
control flow, operators, and module boundaries. The type specification document
covers interfaces, traits, classes, construction, and type member rules. The
project configuration document covers project metadata, dependencies, public
API, library initialization, entry points, and development tooling.

Current slogan
--------------

    Classes store data. Interfaces specify obligations. Traits provide reusable behavior. Factories construct exact classes. Attribute access is structural, not magical. Exports define the public API.
