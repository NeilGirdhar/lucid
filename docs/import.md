# Import

Python already has lazy-import building blocks, such as import hooks and lazy
loaders. Lucid makes laziness the only import behavior instead of an opt-in
building block: every import binds the requested name immediately but does not
load the target module until the name is first used.

```python
import pandas as pd
from .reports import build_report
```
After the first use, the binding behaves like an ordinary import. No separate
keyword or opt-in form is needed.

Lucid has no wildcard import. `from module import *` cannot be lazy even
in principle: binding every name a module exports means already knowing
what those names are, which means the module has to load immediately, the
one shape of import laziness could never cover. Every import names its
targets explicitly instead, so "every import is lazy" holds without a
caveat to remember.
