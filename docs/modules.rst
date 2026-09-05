Modules, projects, and public APIs
=====================================

.. contents:: Table of contents
   :depth: 2
   :local:

Explicit re-exports
------------------------

Packages can re-export public names:

.. code-block:: python

   from .models export User
   from .parsing export parse_user

This keeps the public API local to the definition or re-export site.

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
