# Rejected features

This collects features from other languages — surveyed seriously during
Lucid's design, not dismissed out of hand — that turned out to already
be covered by something else in the language, and so were rejected.
Each entry names the source, what the feature does, and exactly what
already closes the gap it was solving. The point of writing it down is
the same reason a decision, once made, should not need re-deriving
every time a similar-looking proposal resurfaces.

## `once` callback parameters (basedpython)

basedpython marks a callback parameter `once`: the checker verifies
the argument is called exactly once on every path that completes the
function normally, for transaction commits, lock releases, and other
one-shot completion protocols:

```python
def with_transaction(once commit: () -> None):
    do_work()
    commit()
```
Lucid has no such marker, because bracketing already covers every case
where the obligation is checkable at all.

A synchronous exactly-once obligation already has a scope: the
`with`-block itself.

```python
contextmanager def transaction(conn: Connection):
    conn.begin()
    try:
        yield conn
        conn.commit()
    except:
        conn.rollback()
        raise
```
There is no `commit` callback to invoke, so there is nothing to
forget, and nothing to call twice — the block's own boundary is the one
and only commit point, guaranteed the same way `yield` running
exactly once already is ([Context managers](context-managers.md)).

A deferred obligation — call this once a result arrives, potentially
from outside the current call stack entirely — also has a scope. It is
just not a lexical one: it is the coroutine suspended at `await`.
`result = await some_async_operation()` already means "resume this
one suspended activation exactly once, when the result is ready,"
without a callback ever appearing in the caller's own code. What is
left after that — bridging a genuinely external, callback-based API (an
OS completion port, a driver's function-pointer callback) into
something awaitable — is a small, ordinary value, not a discipline
every function needs:

```python
class Once[P: Parameters, R]:
    fn: P -> R
    called: bool = false

    def __call__(self, ***args: P) -> R:
        if self.called:
            raise RuntimeError("called more than once")
        self.called = true
        return self.fn(***args)

    contextmanager classmethod guard(cls, fn: P -> R):
        once = construct(fn)
        yield once
        if not once.called:
            raise RuntimeError("never called")

@Once
def resume(result: Result) -> none:
    ...

driver.register_completion(resume)
```
Bare `@Once` only ever enforces "not called twice" — `called` has
nowhere to raise from if `resume` is simply dropped and never called
at all. Detecting "never called" still needs a scope to check at
closing, but it does not have to be `await`'s: a driver API that
promises to invoke its callback synchronously, before some registering
or pumping call returns, gives `Once.guard` a `with`-block to check
against, closing the gap for exactly that case:

```python
with Once.guard(resume) as guarded:
    driver.register_completion(guarded)
    driver.pump_until_idle()   # resume is guaranteed to fire before this returns
```
For a callback that genuinely fires later, from further outside than
any scope in this function reaches — the fully deferred case `await`
already covers — there is still no boundary to check "never called"
against, and no finalizer to fall back on, for the same non-determinism
reasons already given ([No __del__](classes.md)).

## `local` borrow parameters (basedpython)

basedpython adds a `local` parameter modifier: a value the callee may
use during the call but must not retain past it. Returning it, storing
it on `self`, appending it to an escaping container, or capturing it
in an escaping closure are all compile-time errors, caught by a static
escape analysis:

```python
def f(local fn: () -> None):
    fn()          # ok — used within the call
    return fn     # error: escaping-local
```
The motivating case is a resource-backed value — a view into a buffer,
a file handle — outliving the call it was borrowed for, so a later use
touches something already torn down. Two facts about Lucid, both
already settled for other reasons, close that gap without needing
escape analysis.

`local` is Lucid-only and compile-time-erased, the same as
`abstract`/`override` — it has no jurisdiction across a C ABI
boundary. Native code holding a raw handle was never constrained by it,
so it cannot protect the case that actually matters for an unmanaged
resource: what happens once code outside Lucid touches the value.

And the raw resource is never Lucid-visible to begin with.
[Private members](class-inheritance.md) already keeps a field like a raw file
descriptor out of reach — the only thing visible outside the class is
the wrapping object itself, which can guard its own liveness with an
ordinary stored flag:

```python
class FileHandle:
    _fd: int
    _closed: bool = false

    def read(self) -> bytes:
        if self._closed:
            raise ValueError("read on a closed FileHandle")
        ...
```
Once every access to the resource is mediated through a method that
checks this first, retaining the wrapping object past its
`with`-block is harmless — the worst case is an ordinary leak, not a
dangling read, because the guard sits between every caller and the
resource, unconditionally. Static escape analysis exists to catch what
a runtime guard cannot; here, encapsulation already made the runtime
guard sufficient.

## Receiver-typed callables (basedpython, Kotlin)

basedpython adds an implicit-receiver form for callable types, borrowed
from Kotlin: a type written before the parameter list becomes an
implicit receiver rather than an ordinary parameter, so a value of that
type is available inside the body as `self`/`this`, unqualified:

```python
greet: int.() -> str   # a callable that runs against an int receiver
```
Kotlin uses this for DSL-building — `buildString { append("a");
append("b") }` reads cleanly because the block runs *as* a
`StringBuilder`, not *with* one passed in and qualified on every
call.

The payoff needs one more ingredient Lucid does not have: a
multi-statement block passed inline as an argument. [Anonymous functions](calls.md) are deliberately restricted to a single
expression, to avoid the same "a block that sometimes doubles as a
value" ambiguity `match` already avoids. Without an inline
multi-statement block, receiver typing loses its actual payoff — for a
single expression, `def(sb): sb.append(x)` and whatever a
receiver-typed equivalent would spell are barely different, one
qualifier apart. Anything longer already needs a named, top-level
`def` regardless, and a named function's own `self` is already
explicit and unqualified inside its own body — the same ergonomic win,
achieved the way every other method in Lucid already achieves it.

It is also the one place a parameter would become implicit. Lucid
keeps `self` explicit and named everywhere else, including inside
`contextmanager def __cm__(self):` — an implicit receiver would be
the exception, not an extension of an existing pattern.

## Gradual, any-arity callable type (Python, basedpython)

Python's `Callable[..., R]` (basedpython's `(...) -> R`) types a
callable whose parameter shape is left completely unchecked — accepts
anything, the checker doesn't look. [Function types](types.md)
already rejects this: it is exactly the `Any`-shaped escape hatch
the rest of the type system closes off for everyday code, so Lucid has
no dedicated syntax for it. A genuinely unknown foreign signature
stays `object`, claimed with `trust` like any other untyped value
crossing an interop boundary — recorded here because an `any
Parameters -> R` composition was briefly added to [parameters.md](parameters.md) as a supposedly-free way to spell this, without
checking this decision first, and had to be reverted.

## `and`/`or` as type operators (basedpython)

basedpython accepts the keywords `or`/`and` in annotation
positions as alternate spellings of `|`/`&` — `A or B` means
`A | B`, `A and B` means `A & B` — alongside the symbolic forms,
lowering `and` to a separate `Intersection[...]` generic import
with no native runtime equivalent.

Lucid has neither the alternate spelling nor the separate named type.
[Intersection types](type-operations.md) are `&`, the direct dual of the
`|` already used everywhere for unions; `and`/`or` stay exactly
what they already are, value-level boolean operators, never meaningful
in a type position. Two spellings for the same combinator is exactly
the kind of choice `@overload`'s stub-versus-implementation split
already cost Python — one spelling per idea, not a canonical form and
an alternate a reader also has to learn.

