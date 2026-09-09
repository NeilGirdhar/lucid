# Binary types

`Bytes`, `ByteArray`, and `MemoryView` are capitalized for the same
reason `Bytes`/`ByteArray` already are in [Builtins](builtins.md):
Python's `bytes`, `bytearray`, and `memoryview` stay as the ordinary
lowercase conversion calls, the way `int`/`float`/`str` already do for
their own types, rather than becoming the type names themselves.

`Bytes` is an immutable sequence of raw bytes; `ByteArray` is its
mutable counterpart — the one built-in pair in Python where the
immutable variant, not the mutable one, is the default name, the
opposite of `list`/`!list`'s own convention. Lucid keeps that pairing
as two distinct types rather than folding it into the ordinary
mutable/immutable view system `~T`/`!T` already gives every other
type: the two already have Python's own separate identities and
separate conventional uses, and unifying them is a bigger design
question than renaming them.

```python
data: Bytes = b"hello"
buffer: ByteArray = bytearray(b"hello")
buffer[0] = 72
```
`MemoryView` is a view over an existing buffer without copying it —
`Bytes`, `ByteArray`, or anything else that exposes the buffer
protocol. Slicing a `MemoryView` produces another `MemoryView` over
the same underlying storage, not a copy:

```python
view: MemoryView = memoryview(buffer)
window: MemoryView = view[2:5]  # still backed by buffer, no copy
```
