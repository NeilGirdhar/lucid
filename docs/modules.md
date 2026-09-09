# Module-private names

A name starting with `_` is private to where it is declared: a leading
underscore on a definition at the top level of a file keeps it out of
every other file in the project. Everything else is visible project-wide
by default — no keyword marks it public.

```python
def _parse_line(line: str) -> Row:        # this file only
    ...

def parse_file(path: Path) -> list[Row]:  # every file in the project
    ...
```
A member named with a leading `_` follows the same rule one level
down, private to its class instead of its file; see
[Private members](class-inheritance.md).

This is Python's own convention, finally enforced. Python's single
leading underscore is a request nothing checks — `from module import
_private` already works regardless. Lucid closes that gap: reaching for
a private name is a checked error, the same way everywhere it can
happen.

Which of a project's names are visible *outside* it, to another project
depending on this one, is a different question with one answer, in one
place: [Public API](project-configuration.md).

## No `__all__`

`__all__` only narrows what `from module import *` sees; a direct
import of anything else in the module reaches it regardless, so a
module's real public surface and its `__all__` list can say two
different things. Lucid has no wildcard import for a second list to
narrow in the first place (see [Import](import.md)), and the one list
that does exist — private names decided by a leading `_`, checked
everywhere — cannot fall out of sync with itself the way a
separately-maintained `__all__` can with the module it describes.
