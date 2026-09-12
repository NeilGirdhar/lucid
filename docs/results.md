# Results

Python collapses two different kinds of failure into one mechanism.
`raise`/`try`/`except` handle both "this call can fail in an
ordinary, expected way" — a missing key, a parse failure — and "something
is broken" — a violated invariant, a bug — and because exceptions carry no
signature, nothing about a function's type distinguishes the two. Java
tried to fix the visibility problem by making exceptions checked —
declared in `throws`, enforced by the compiler — and got the goal right
and the mechanism wrong: checked exceptions do not compose with generics
or lambdas, and one new exception type deep in a call chain forces every
intermediate signature to change, which is why nearly every language
designed since has rejected the mechanism while still agreeing with what
it was reaching for.

Lucid splits the two kinds of failure instead of choosing one mechanism
for both: this covers the expected, recoverable kind; [Exceptions](exceptions.md)
covers the other.

## Recoverable errors are ordinary return types

An expected, recoverable failure is just part of what a function returns —
a union, handled the same exhaustive way any other union is:

```python
type ParseResult = Value | ParseError

def parse(text: str) -> ParseResult:
    ...

match parse(text) as result:
    case Value:
        ...
    case ParseError:
        ...
```
This needs no new mechanism: it is [Exhaustive pattern matching](match.md#exhaustive-pattern-matching) applied
to errors, not a separate error-handling feature. The checker already
refuses to let a case go unhandled, which is the guarantee Java's checked
exceptions were reaching for, without the signature-propagation cost —
adding a new failure mode to `ParseResult` is an ordinary union change,
not something every caller up the chain has to redeclare.

Matching every fallible call by hand does not scale —
[The `?` operator](question-mark-operator.md) covers the sugar for
propagating one of these without a `match` at every call site.
