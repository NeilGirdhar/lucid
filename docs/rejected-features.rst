Rejected features
====================

.. contents:: Table of contents
   :depth: 2
   :local:

This collects features from other languages — surveyed seriously during
Lucid's design, not dismissed out of hand — that turned out to already
be covered by something else in the language, and so were rejected.
Each entry names the source, what the feature does, and exactly what
already closes the gap it was solving. The point of writing it down is
the same reason a decision, once made, should not need re-deriving
every time a similar-looking proposal resurfaces.

``once`` callback parameters (basedpython)
--------------------------------------------------

basedpython marks a callback parameter ``once``: the checker verifies
the argument is called exactly once on every path that completes the
function normally, for transaction commits, lock releases, and other
one-shot completion protocols:

.. code-block:: python

   def with_transaction(once commit: () -> None):
       do_work()
       commit()

Lucid has no such marker, because bracketing already covers every case
where the obligation is checkable at all.

A synchronous exactly-once obligation already has a scope: the
``with``-block itself.

.. code-block:: python

   contextmanager def transaction(conn: Connection):
       conn.begin()
       try:
           yield conn
           conn.commit()
       except:
           conn.rollback()
           raise

There is no ``commit`` callback to invoke, so there is nothing to
forget, and nothing to call twice — the block's own boundary is the one
and only commit point, guaranteed the same way ``yield`` running
exactly once already is (`Context managers <context-managers.rst>`_).

A deferred obligation — call this once a result arrives, potentially
from outside the current call stack entirely — also has a scope. It is
just not a lexical one: it is the coroutine suspended at ``await``.
``result = await some_async_operation()`` already means "resume this
one suspended activation exactly once, when the result is ready,"
without a callback ever appearing in the caller's own code. What is
left after that — bridging a genuinely external, callback-based API (an
OS completion port, a driver's function-pointer callback) into
something awaitable — is a small, ordinary value, not a discipline
every function needs:

.. code-block:: python

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

Bare ``@Once`` only ever enforces "not called twice" — ``called`` has
nowhere to raise from if ``resume`` is simply dropped and never called
at all. Detecting "never called" still needs a scope to check at
closing, but it does not have to be ``await``'s: a driver API that
promises to invoke its callback synchronously, before some registering
or pumping call returns, gives ``Once.guard`` a ``with``-block to check
against, closing the gap for exactly that case:

.. code-block:: python

   with Once.guard(resume) as guarded:
       driver.register_completion(guarded)
       driver.pump_until_idle()   # resume is guaranteed to fire before this returns

For a callback that genuinely fires later, from further outside than
any scope in this function reaches — the fully deferred case ``await``
already covers — there is still no boundary to check "never called"
against, and no finalizer to fall back on, for the same non-determinism
reasons already given (`No __del__ <classes.rst>`_).

``local`` borrow parameters (basedpython)
-------------------------------------------------

basedpython adds a ``local`` parameter modifier: a value the callee may
use during the call but must not retain past it. Returning it, storing
it on ``self``, appending it to an escaping container, or capturing it
in an escaping closure are all compile-time errors, caught by a static
escape analysis:

.. code-block:: python

   def f(local fn: () -> None):
       fn()          # ok — used within the call
       return fn     # error: escaping-local

The motivating case is a resource-backed value — a view into a buffer,
a file handle — outliving the call it was borrowed for, so a later use
touches something already torn down. Two facts about Lucid, both
already settled for other reasons, close that gap without needing
escape analysis.

``local`` is Lucid-only and compile-time-erased, the same as
``abstract``/``override`` — it has no jurisdiction across a C ABI
boundary. Native code holding a raw handle was never constrained by it,
so it cannot protect the case that actually matters for an unmanaged
resource: what happens once code outside Lucid touches the value.

And the raw resource is never Lucid-visible to begin with.
`Private members <classes.rst>`_ already keeps a field like a raw file
descriptor out of reach — the only thing visible outside the class is
the wrapping object itself, which can guard its own liveness with an
ordinary stored flag:

.. code-block:: python

   class FileHandle:
       _fd: int
       _closed: bool = false

       def read(self) -> bytes:
           if self._closed:
               raise ValueError("read on a closed FileHandle")
           ...

Once every access to the resource is mediated through a method that
checks this first, retaining the wrapping object past its
``with``-block is harmless — the worst case is an ordinary leak, not a
dangling read, because the guard sits between every caller and the
resource, unconditionally. Static escape analysis exists to catch what
a runtime guard cannot; here, encapsulation already made the runtime
guard sufficient.
