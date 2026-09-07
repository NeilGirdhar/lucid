Context managers
==================

.. contents:: Table of contents
   :depth: 2
   :local:

Two unrelated protocols
---------------------------

Python has two ways to write a context manager, and neither is simple.
The raw protocol needs two methods with an awkward, easy-to-misuse shape:

.. code-block:: python

   class Lock:
       def __enter__(self):
           self._acquire()
           return self

       def __exit__(self, exc_type, exc_val, exc_tb):
           self._release()
           return False   # forgetting this, or returning anything else
                           # truthy, silently swallows the exception

``__exit__``'s three exception-info parameters exist only so it can
decide whether to suppress the exception, and it decides that by
*return value* — a convention easy to get wrong by accident, since any
stray truthy return silently swallows an exception nobody meant to
catch.

The shortcut, ``@contextmanager``, avoids writing ``__exit__`` by hand,
at the cost of a decorator, a generator function, and a
``try``/``finally`` the author has to remember unprompted:

.. code-block:: python

   from contextlib import contextmanager

   @contextmanager
   def locked(lock):
       lock.acquire()
       try:
           yield lock
       finally:
           lock.release()

Forget the ``try``/``finally`` and the bug is silent: ``lock.release()``
simply never runs if the ``with``-block raises, because an exception
injected at a bare ``yield`` propagates straight out of the generator
with nothing left to run afterward. Two protocols exist because the
second was bolted onto the first later, not because either is the
obviously right shape for the problem — the same situation
`Modern type specification <type-specification.rst>`_ describes for
Python's own class-specification tools, one level down.

One modifier, one shape
----------------------------

Lucid has one way to write a context manager: ``contextmanager``, a
modifier that stacks in front of ``def`` or ``classmethod`` the same way
`Explicit overrides <traits.rst>`_'s ``override`` already does, rather
than replacing it the way ``classmethod``, ``factory``, ``getter``, and
``setter`` do:

.. code-block:: python

   contextmanager def locked(lock: Lock):
       lock.acquire()
       yield lock
       lock.release()

Code before ``yield`` is setup; the yielded value is what ``as`` binds;
code after ``yield`` is teardown. Nothing here is a generator in the
sense that matters to a reader — ``yield`` still means "suspend here,
hand a value across, resume later," the same meaning it already has —
but a ``contextmanager`` body yields exactly once, checked, not left to
convention the way Python's version is.

Guaranteed cleanup needs no ``try``
------------------------------------------

If ``yield`` is not already inside an explicit ``try``, the compiler
wraps everything after it in an implicit ``finally``: teardown runs
whether the ``with``-block raised or not, with nothing written by hand
to guarantee it.

.. code-block:: python

   contextmanager def locked(lock: Lock):
       lock.acquire()
       yield lock
       lock.release()   # always runs -- no try/finally needed to say so

This is the common case, and it needed a ``try``/``finally`` in Python
only because nothing else would run the cleanup for you. Here, not
writing one is not a bug waiting to happen; it is what the modifier
already means.

Explicit ``try`` for anything more
----------------------------------------

Wrapping ``yield`` in an explicit ``try`` opts back into full control,
using nothing beyond ordinary ``try``/``except``/``raise`` (see
`Control flow and statements <control-flow.rst>`_) — no separate
suppression mechanism, and no return value doing double duty the way
``__exit__``'s does:

.. code-block:: python

   contextmanager def transaction(conn: Connection):
       conn.begin()
       try:
           yield conn
           conn.commit()
       except:
           conn.rollback()
           raise

A caught exception that is re-raised still propagates after cleanup
runs; one that is not re-raised is suppressed — gone, the same way any
other ``except`` without a ``raise`` already discards what it catches.
Suppression is visible at the exact ``except`` clause that causes it,
never a side effect of some unrelated return value.

Making a class itself usable in ``with``
-----------------------------------------------

``with x:`` — no call, the instance itself managed — desugars to
``with x.__cm__():``. A class opts in by declaring ``contextmanager def
__cm__(self):``, folding Python's separate ``__enter__``/``__exit__``
protocol into the same modifier used everywhere else, rather than
asking an author to learn a second protocol for this one case:

.. code-block:: python

   class Session:
       conn: Connection

       contextmanager def __cm__(self):
           self.conn.begin()
           yield self
           self.conn.commit()

   with Session(conn):
       ...

``__cm__`` is dunder-spelled for the same reason ``__eq__`` is, not the
reason ``__len__`` is (see `Numeric types <numeric-types.rst>`_'s
``Sized``): it is tied to ``with`` syntax specifically, not competing
with an ordinary word a class might separately want for something else.

``classmethod`` combines the same way
--------------------------------------------

A resource that is both constructed and managed — ``open``, ``connect``,
``acquire`` — needs both modifiers at once, ``contextmanager`` always
first, the same order whether the other modifier is ``def`` or
``classmethod``:

.. code-block:: python

   class Session:
       conn: Connection

       contextmanager classmethod open(cls, path: str) -> Session:
           conn = Connection.connect(path)
           yield Session(conn)
           conn.close()

   with Session.open("db.sqlite") as s:
       ...

``contextmanager`` does not combine with ``getter``, ``setter``, or
``factory``. A getter's contract ends at "compute and return a value,"
and a factory's ends at "return a fully constructed object" — neither
has anywhere for a "resume after the block" to attach to the way ``def``
and ``classmethod`` do.

``with`` itself is unchanged
-----------------------------------

None of this touches the calling side: ``with expr:`` and
``with expr as name:`` work exactly as they already do, on anything
``contextmanager`` produces or any class declaring
``contextmanager def __cm__(self):``.

No exactly-once callback parameter
------------------------------------------

Some languages give a callback parameter its own exactly-once
obligation instead — a marker meaning "the checker verifies this
argument is called exactly once on every path that completes the
function normally," for transaction commits, lock releases, and other
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
forget, and nothing to call twice — the block's own boundary is the
one and only commit point, guaranteed the same way ``yield`` running
exactly once already is.

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

So every real "make sure this runs exactly once" reduces to a scope
Lucid already has: synchronous, ``contextmanager``; asynchronous,
``await``. A callback parameter marked exactly-once would only restate
one of the two, with a weaker guarantee at the far end.
