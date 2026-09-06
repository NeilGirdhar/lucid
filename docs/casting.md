# Casting

A Python value crossing into Lucid with no further information is typed as
`object` — nothing is assumed about it, the same as any other value whose
shape is genuinely unknown. That is not a new rule; it is the
[no-`Any` principle](types.md#no-any-escape-hatch) applied to
values that happen to come from outside Lucid, instead of values that
happen to be under-specified inside it.

Getting anything more specific out of an `object` that actually came from
Python requires an explicit, unverified claim: `trust`.

```python
raw: object = some_python_function()
items: !list[int] = trust[!list[int]](raw)
```
`trust` asserts a type with no proof behind it — there is nothing on the
Python side for the checker to verify against — but the claim is visible,
written once, at the exact place it is made. That is the difference from
`Any`: `Any` lets a value be used as anything, anywhere, with no marker
recording that a leap was taken; `trust` requires writing down exactly
what is being trusted, and where, every time.

`trust` only accepts an `object`-typed operand. Python's `typing.cast`
has no such restriction — it can assert any type in place of any other,
anywhere, purely between values that are already fully typed on the Python
side, with nothing foreign involved at all. Lucid has no equivalent
general-purpose `cast`, on purpose: traits are nominal specifically so
that satisfying one is an explicit, checked act rather than an accidental
shape match, and an unrestricted cast would let any code route around that
check between two ordinary, already-sound Lucid values — `trust[Dog](some_cat)`
between two well-typed Lucid values is not filling a real gap, it is
punching a hole where the checker already had real information. Restricting
`trust` to `object` operands makes that impossible by construction: it
only ever gets to speak where the checker had nothing to say in the first
place, which is exactly and only the Python interop boundary.

Calling `trust` at every use site does not scale to an entire library.
Instead, foreign Python modules can be typed at import time using `.pyi` type
stubs or bindings declared in `project.yaml` (see
[Project configuration](project-configuration.md)). Once typed at the
import boundary, values entering Lucid carry verified static types across the
rest of the program without per-call `trust` annotations.

## Toll-free Python 3.13+ ABI bridging

Foreign interop in alternative language runtimes often introduces steep
performance penalties: systems like PyPy or GraalPy historically paid a 2x–10x
slowdown when crossing into C extensions because their internal object layouts
diverged from CPython, requiring runtime proxy allocation, pointer pinning, and
bidirectional state synchronization.

Lucid targets Python 3.13+ exclusively and avoids that penalty through
*toll-free ABI bridging*: every Lucid heap allocation shares the binary prefix
of CPython's `PyObject` (reference count and type descriptor pointer).
Because the memory layout matches, passing a Lucid object to a C extension or
receiving one back requires no translation, no shadow wrapper, and no copying.
The C extension dereferences standard fields and macros
(`PyList_GET_ITEM`, `PyTuple_GET_ITEM`) directly against Lucid memory.

Calls and data sharing exploit three modern Python 3.13 runtime features:

- **Free-threading (PEP 703)**: Lucid targets Python 3.13's free-threaded
  (`nogil`) runtime. Multithreaded Lucid programs run across all CPU cores in
  true parallel without acquiring a Global Interpreter Lock when calling foreign
  Python or C code.
- **Immortal objects (PEP 683)**: Lucid's transitively frozen `!T` values
  ([Mutability](mutability.md)) map directly to Python 3.13 immortal
  objects. Because immortal objects have fixed reference counts that the runtime
  never modifies, foreign Python and C code can share frozen Lucid values across
  threads without atomic reference-counting contention or cache-line bouncing.
- **Vectorcall and the buffer protocol**: Argument passing across the foreign
  boundary uses Python 3.13's standardized vectorcall convention
  (`PyObject_Vectorcall`), passing arguments via contiguous stack-allocated
  pointer arrays with zero temporary heap allocation. Array and tensor data
  share contiguous memory directly with libraries such as NumPy and PyTorch
  through the C buffer protocol (`Py_buffer`).

With core type expressions and the interop boundary established, the next
question is how types constrain mutation — covered next in
[Mutability](mutability.md).
