# Files

## The mode string typeshed can't type

Python's `open(path, mode, ...)` picks one of several unrelated return
types — `TextIOWrapper`, `BufferedReader`, `BufferedWriter`,
`BufferedRandom`, `FileIO` — from the *value* of a string argument,
not its type. Typeshed's stub is the result: dozens of `@overload`
declarations, one per `Literal["r"]`, `Literal["rb"]`,
`Literal["r+b"]`, `Literal["br"]`, and every other order a mode's
letters can appear in, because `"rb"` and `"br"` are both valid and
mean the same thing. The moment `mode` is an ordinary `str` computed
at runtime — built with `+`, read from configuration, chosen by an
`if` — none of those `Literal` overloads match, and the whole call
falls back to a final catch-all overload returning `IO[Any]`. The one
line that actually opens the file is the one line the type checker
stops checking, no matter how the rest of the program is annotated —
[the same disease `**` has](numeric-types.md#pow-dispatches-per-type),
in a different part of the standard library.

There is no bare `open` in Lucid ([Removed builtins](removed-builtins.md));
`Path.open` replaces it, dispatching on argument *types* instead of a
string's contents, so every combination has its own real return type.

## `Path.open`

`Path.open` takes two markers instead of a mode string — an encoding,
`Text` or `Binary`, and an access mode, `Read`, `Write`, or `Update` —
each an ordinary, fieldless class rather than a string fragment, so
[multiple dispatch](dispatch.md) resolves the exact return type from
the arguments' types, the same way `pow` resolves on `int` versus
`float` versus `complex`:

```python
class Text: ...
class Binary: ...
class Read: ...
class Write: ...
class Update: ...

contextmanager def dispatch open(path: Path, encoding: Text, mode: Read) -> TextReader:
    handle = ...  # the OS-level open call
    yield TextReader(handle)
    handle.close()

contextmanager def dispatch open(path: Path, encoding: Binary, mode: Write) -> BinaryWriter:
    handle = ...
    yield BinaryWriter(handle)
    handle.close()

contextmanager def dispatch open(path: Path, encoding: Text, mode: Update) -> TextUpdater:
    handle = ...
    yield TextUpdater(handle)
    handle.close()

# ... Binary/Read, Text/Write, Binary/Update follow the same pattern
```
```python
with path.open(Text(), Write()) as f:
    f.write("Hello, Lucid")
```
No case needs a runtime check of which mode was requested — each is
its own definition, so `TextReader.read() -> str` and
`BinaryReader.read() -> bytes` are just what their signatures already
say, never a `str | bytes` a caller has to narrow by hand the way
Python's own `IO[Any]` return forces.

## Always a context manager, never a bare handle

`contextmanager` is not a detail of how `Path.open` happens to be
implemented; it is the entire fix. Three things Python's `open` gets
wrong follow directly from it, the way [Context
managers](context-managers.md#classmethod-combines-the-same-way)
already establishes for any constructed, managed resource:

- **The call alone opens nothing.** `path.open(Text(), Read())`
  constructs the context manager value; none of its body runs until a
  `with` block enters it, the same way calling a generator function
  builds a generator without running a single line of its body.
  Python's `open()` touches the filesystem immediately, on the call
  itself — there is no way to build an open request without it also
  being an open file.
- **There is no `close` method to forget.** The object a `with` block
  binds — `TextReader`, `BinaryWriter`, whichever case matched — is
  never given one; the actual close call is teardown code inside
  `Path.open` itself, after `yield`, and it runs on the way out of the
  block whether the block raised or not
  ([Guaranteed cleanup needs no `try`](context-managers.md#guaranteed-cleanup-needs-no-try)).
  A reader or writer with no `close` method can't be closed twice,
  closed early, or forgotten — Python's own most common file-handling
  bug has no method left to call by mistake.
- **The handle can't outlive the block.** Since the only way to
  produce one is through `with`, nothing can hold a `TextReader` past
  the point its underlying file is closed the way a Python file object
  can be squirreled away, used later, and found already closed.

## Reading or writing a whole file at once

For the common case — read everything, write everything, no streaming
— `read_file(path)` and `write_file(path, contents: str)` skip
`Path.open` entirely: a plain function call returning the contents (or
`none`) alongside a recoverable failure case, handled exhaustively the
way [Results](results.md) covers, no `with` block for a resource
that's already closed by the time the call returns:

```python
contents = read_file(path)?
write_file(path, contents.upper())?
```
`Path.open`'s markers and dispatch exist for the streaming case these
two calls are shortcuts around, not a replacement for it.
