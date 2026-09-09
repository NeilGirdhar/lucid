# With

`with expr:` and `with expr as name:` are the calling side of Lucid's
context-manager machinery, and neither changes from Python: both work
exactly as they already do, on anything
[Context managers](context-managers.md)'s `contextmanager` produces,
or any class declaring `contextmanager def __cm__(self):`.

This bracketing is also why Lucid has no exactly-once callback-parameter
marker, the way some languages do for transaction commits and one-shot
completion handlers — see [Rejected features](rejected-features.md).

So every real "make sure this runs exactly once" reduces to a scope
Lucid already has: synchronous, `contextmanager`; asynchronous,
`await`. A callback parameter marked exactly-once would only restate
one of the two, with a weaker guarantee at the far end.
