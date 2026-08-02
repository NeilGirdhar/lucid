Project configuration
======================

A Lucid project is configured by two StrictYAML files at its root:
``project.yaml`` and ``development.yaml``. Between them, these two files take
over the roles Python spreads across ``pyproject.toml``, ``setup.cfg``, and
the side-effect code conventionally placed in ``__init__.py``.

``project.yaml`` describes the project as a dependency: what it needs to run,
what it exposes to other code, and how it is initialized and invoked.
``development.yaml`` describes the project as a workspace: the tools used to
build, check, and format it, and the extra dependencies only a contributor
needs. A consumer that only installs and runs a project never needs
``development.yaml``.

.. contents:: Table of contents
   :depth: 3
   :local:

``project.yaml``
-----------------

Project identity
~~~~~~~~~~~~~~~~~

``project.yaml`` opens with the same descriptive metadata that
``pyproject.toml``'s ``[project]`` table carries: a name, a version, a short
description, and a pointer to the readme file.

.. code-block:: yaml

   name: acme-inference
   version: "2.3.0"
   description: Inference serving for Acme models
   readme: README.rst

A field can be left out of ``project.yaml`` and listed under ``dynamic``
instead, when its value is computed rather than written by hand, for example a
version stamped in from a version-control tag:

.. code-block:: yaml

   dynamic:
     - version

Lucid version
~~~~~~~~~~~~~

``lucid`` is the required-``requires-python``: a version specifier for the
Lucid toolchain the project needs.

.. code-block:: yaml

   lucid: ">=0.4"

Because ``lucid`` names the toolchain that both runs and builds a project,
``project.yaml`` has no counterpart to ``pyproject.toml``'s ``[build-system]``
table. There is no separate build backend to select or pin; the ``lucid``
version specifier already pins the one toolchain that builds and runs the
project.

License
~~~~~~~

``license`` is an SPDX license expression, and ``license-files`` is a list of
glob patterns for the license text to distribute, following the same model as
the modern ``pyproject.toml`` license fields:

.. code-block:: yaml

   license: Apache-2.0
   license-files:
     - LICENSE

People and links
~~~~~~~~~~~~~~~~~

``authors`` and ``maintainers`` are lists of name/email pairs. ``keywords``
and ``classifiers`` describe the project for a package index. ``urls`` is a
mapping of labels to links:

.. code-block:: yaml

   authors:
     - name: Ada Lovelace
       email: ada@example.com
   maintainers:
     - name: Grace Hopper
       email: grace@example.com
   keywords:
     - inference
     - serving
   classifiers:
     - "Intended Audience :: Developers"
   urls:
     Homepage: https://example.com/acme-inference
     Documentation: https://example.com/acme-inference/docs
     Repository: https://example.com/acme-inference.git
     Issues: https://example.com/acme-inference/issues

Dependencies
~~~~~~~~~~~~

``dependencies`` is a mapping from a required project's name to a version
specifier, in place of ``pyproject.toml``'s flat list of requirement strings:

.. code-block:: yaml

   dependencies:
     numpy: ">=1.26"
     acme-models: ">=1.0,<2.0"

This is more than a syntax choice: the same names and specifiers are what
``library.initialize`` walks to topologically sort library initialization (see
`Library initialization`_ below), so a project's dependency list and its
initialization order come from one declaration instead of two.

``optional-dependencies`` declares installable extras the same way
``pyproject.toml`` does: named groups of additional dependencies that a
consumer can opt into.

.. code-block:: yaml

   optional-dependencies:
     gpu:
       cupy: ">=13.0"

Dependencies that only a contributor to the project needs, such as test or
lint tooling, do not belong here; they are declared in ``development.yaml``'s
`Development dependencies`_.

Public API
~~~~~~~~~~

``export`` is a tree that mirrors the paths a project makes public, the
project-level counterpart to the ``export`` keyword used inside a module:

.. code-block:: yaml

   export:
     models:
       User: .models.User
     parsing:
       parse_user: .parsing.parse_user

Local aliases
~~~~~~~~~~~~~

``local-alias`` declares a tree of aliases that remap paths for local use
inside the project. Aliases can flatten an internal source layout, for example
mapping an internal ``._src`` package back onto the project root, or promote a
deeply nested definition to a short local path:

.. code-block:: yaml

   local-alias:
     _src: .
     matrix: .some.deep.path.matrix

Between ``export`` and ``local-alias``, a project has no remaining need for
``__init__.py``-style re-exporting or flattening modules.

Library initialization
~~~~~~~~~~~~~~~~~~~~~~~

``library-context`` names a context manager, holding the setup work that
Python would otherwise run as side effects in ``__init__.py``, such as
configuring an underlying native library:

.. code-block:: yaml

   library-context: .setup.initialize

Because imports are lazy, that setup does not run just because a name is
imported. It runs only when ``library.initialize`` enters it:

.. code-block:: python

   with library.initialize({"numpy": numpy_params, "acme-inference": project_params}):
       run_entry_point()

Using a library's behavior without its context manager having been entered is
an error. Each named library's ``library-context`` is entered in dependency
order: ``library.initialize`` topologically sorts the libraries being
initialized using the ``dependencies`` declared in each project's
``project.yaml``, so a library is initialized after every library it depends
on.

Entry points
~~~~~~~~~~~~

``entry-points`` is a mapping of names to paths: the runnable commands a
project exposes, for example to the command line, in place of
``pyproject.toml``'s ``[project.scripts]`` table:

.. code-block:: yaml

   entry-points:
     serve: .cli.serve
     migrate: .cli.migrate

Running an entry point is what opens ``library.initialize``; see
`Library initialization`_.

``development.yaml``
---------------------

Tool configuration
~~~~~~~~~~~~~~~~~~~

``pyproject.toml`` accumulates one ``[tool.X]`` table per tool a project uses.
Lucid keeps that configuration out of ``project.yaml`` entirely and collects it
under ``tools`` in ``development.yaml``, keyed by tool name:

.. code-block:: yaml

   tools:
     formatter:
       line-length: 88
     linter:
       select:
         - unused-import
         - undefined-name
         - unsorted-imports
         - non-pep585-annotation
       ignore:
         - line-too-long
         - missing-trailing-comma

Development dependencies
~~~~~~~~~~~~~~~~~~~~~~~~~

``dependency-groups`` declares named groups of dependencies that only a
contributor needs: test runners, linters, documentation builders. Unlike
``project.yaml``'s ``dependencies`` and ``optional-dependencies``, these are
never installed for a consumer of the project and are never part of what
``library.initialize`` initializes. A group can include another group:

.. code-block:: yaml

   dependency-groups:
     test:
       pytest: ">=8.0"
     lint:
       ruff: ">=0.5"
     dev:
       - include-group: test
       - include-group: lint

Correspondence with ``pyproject.toml``
---------------------------------------

.. list-table::
   :header-rows: 1

   * - ``pyproject.toml``
     - Lucid
   * - ``[project] name``, ``version``, ``description``, ``readme``
     - same, in ``project.yaml``
   * - ``[project] requires-python``
     - ``project.yaml``'s ``lucid``
   * - ``[project] license``, ``license-files``
     - same, in ``project.yaml``
   * - ``[project] authors``, ``maintainers``, ``keywords``, ``classifiers``, ``urls``
     - same, in ``project.yaml``
   * - ``[project] dependencies``
     - ``project.yaml``'s ``dependencies`` (a name/specifier mapping, also
       used for library initialization order)
   * - ``[project] optional-dependencies``
     - same, in ``project.yaml``
   * - ``[project] dynamic``
     - same, in ``project.yaml``
   * - ``[project] scripts``, ``gui-scripts``
     - ``project.yaml``'s ``entry-points``
   * - ``[build-system]``
     - not needed; the ``lucid`` version specifier pins the one toolchain
   * - ``__init__.py`` side effects
     - ``project.yaml``'s ``library-context``, run by ``library.initialize``
   * - ``__init__.py`` re-exports
     - ``project.yaml``'s ``export`` and ``local-alias``
   * - ``[dependency-groups]``
     - ``development.yaml``'s ``dependency-groups``
   * - ``[tool.X]`` tables
     - ``development.yaml``'s ``tools``

Full example
------------

.. code-block:: yaml

   # project.yaml
   name: acme-inference
   version: "2.3.0"
   description: Inference serving for Acme models
   readme: README.rst
   lucid: ">=0.4"
   license: Apache-2.0
   license-files:
     - LICENSE
   authors:
     - name: Ada Lovelace
       email: ada@example.com
   urls:
     Homepage: https://example.com/acme-inference
     Repository: https://example.com/acme-inference.git
   dependencies:
     numpy: ">=1.26"
     acme-models: ">=1.0,<2.0"
   optional-dependencies:
     gpu:
       cupy: ">=13.0"
   export:
     models:
       User: .models.User
     parsing:
       parse_user: .parsing.parse_user
   local-alias:
     _src: .
     matrix: .some.deep.path.matrix
   library-context: .setup.initialize
   entry-points:
     serve: .cli.serve
     migrate: .cli.migrate

.. code-block:: yaml

   # development.yaml
   dependency-groups:
     test:
       pytest: ">=8.0"
     lint:
       ruff: ">=0.5"
     dev:
       - include-group: test
       - include-group: lint
   tools:
     formatter:
       line-length: 88
     linter:
       select:
         - unused-import
         - undefined-name
         - unsorted-imports
         - non-pep585-annotation
       ignore:
         - line-too-long
         - missing-trailing-comma
