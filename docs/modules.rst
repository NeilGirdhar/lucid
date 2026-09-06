Modules, projects, and public APIs
=====================================

.. contents:: Table of contents
   :depth: 2
   :local:

Module-private names
------------------------

A name starting with ``_`` is private to where it is declared: a leading
underscore on a definition at the top level of a file keeps it out of
every other file in the project. Everything else is visible project-wide
by default — no keyword marks it public.

.. code-block:: python

   def _parse_line(line: str) -> Row:        # this file only
       ...

   def parse_file(path: Path) -> list[Row]:  # every file in the project
       ...

A member named with a leading ``_`` follows the same rule one level
down, private to its class instead of its file; see
`Private members <classes.rst>`_.

This is Python's own convention, finally enforced. Python's single
leading underscore is a request nothing checks — ``from module import
_private`` already works — and ``__all__`` only narrows what ``from
module import *`` sees, leaving a direct import of anything else
untouched. Lucid has neither gap: reaching for a private name is a
checked error, the same way everywhere it can happen, with no second,
separately maintained list to fall out of sync with the first.

Which of a project's names are visible *outside* it, to another project
depending on this one, is a different question with one answer, in one
place: `Public API <project-configuration.rst>`_.

Lazy imports
----------------

Python already has lazy-import building blocks, such as import hooks and lazy
loaders. Lucid makes laziness the only import behavior instead of an opt-in
building block: every import binds the requested name immediately but does not
load the target module until the name is first used.

.. code-block:: python

   import pandas as pd
   from .reports import build_report

After the first use, the binding behaves like an ordinary import. No separate
keyword or opt-in form is needed.

Lucid has no wildcard import. ``from module import *`` cannot be lazy even
in principle: binding every name a module exports means already knowing
what those names are, which means the module has to load immediately, the
one shape of import laziness could never cover. Every import names its
targets explicitly instead, so "every import is lazy" holds without a
caveat to remember.

Projects
------------

Every project has a ``project.yaml`` file, written in StrictYAML, that
declares the project's identity and dependencies, its public API, its local
import shortcuts, its library initialization, and its runnable commands. A
sibling ``development.yaml`` holds tool configuration and development-only
dependencies. Together they take over the roles Python splits across
``pyproject.toml`` and ``__init__.py``.

Lazy imports mean none of this runs implicit setup code on ``import``: a
project's library initialization runs only when an entry point explicitly
opens ``library.initialize`` for the libraries it depends on. The full
structure of both files is covered separately in
`Project configuration <project-configuration.rst>`__.
