# Binary types

`Bytes`, `ByteArray`, and `MemoryView` are ordinary classes, capitalized
like any other, and constructed the same way any other class is —
through their own default factory. Python spells the same three
constructions `bytes(x)`, `bytearray(x)`, and `memoryview(x)`, separate
lowercase functions distinct from the type names; Lucid has no second
name to reach for, since `Bytes(x)` already is the construction call.

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
buffer: ByteArray = ByteArray(b"hello")
buffer[0] = 72
```
`MemoryView` is a view over an existing buffer without copying it —
`Bytes`, `ByteArray`, or anything else that exposes the buffer
protocol. Slicing a `MemoryView` produces another `MemoryView` over
the same underlying storage, not a copy:

```python
view: MemoryView = MemoryView(buffer)
window: MemoryView = view[2:5]  # still backed by buffer, no copy
```
"Anything else that exposes the buffer protocol" is `Buffer`, the
capability trait that names the protocol itself, rather than a fixed
list of three built-in types:

```python
trait Buffer:
    def __buffer__(self: ~Self) -> MemoryView
```

`Bytes`, `ByteArray`, and `MemoryView` all satisfy `Buffer`. A
third-party class wrapping something else buffer-shaped — a
memory-mapped file, say — can satisfy it too, the same way
[Implementing a trait after the fact](traits.md#implementing-a-trait-after-the-fact)
lets any existing type pick up a capability it did not originally
declare.
