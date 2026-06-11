5. The import system
====================

5.1. Imports and exports
------------------------

Packages can re-export public names:

.. code-block:: python

   export from .models import User
   export from .parsing import parse_user

This keeps the public API local to the definition or re-export site.

5.2. Lazy imports
-----------------

Imports can be marked lazy when a dependency is expensive or optional.

.. code-block:: python

   lazy import pandas as pd
   lazy from .reports import build_report

A lazy import binds the requested name immediately, but does not load the target module until the name is first used. After the first use, the binding behaves like an ordinary import.
