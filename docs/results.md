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

match parse(text):
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

## The `?` operator

Matching every fallible call by hand does not scale. Go's version of this
problem is `if err != nil { return nil, err }` repeated after nearly
every call — legible, but the boilerplate outweighs the logic it surrounds.
`?` is sugar for the common case: propagate the error case unchanged, or
unwrap to the value and keep going.

```python
def load(path: str) -> Config | ParseError:
    text = read_file(path)?
    return parse(text)?
```
`text = read_file(path)?` means exactly:

```python
match read_file(path) as outcome:
    case ParseError:
        return outcome
    case _:
        text = outcome
```
`?` requires the enclosing function's own return type to already accept
whatever error type is being propagated — `load`'s return type has to
name `ParseError` for either `?` above to be legal, the same
requirement Rust's `?` places on the enclosing function's error type.

This is where Lucid lands between Python and Rust, and closer to Rust.
Python's exceptions propagate automatically with no mark anywhere in the
source that a given call might fail — reading a function's signature tells
you nothing about what it might raise. Lucid's `?` also propagates
automatically, but every place it happens is a literal character in the
source, visible and searchable, the same as Rust's — nothing is invisible:
the return type says what can go wrong, and each `?` says exactly where
a failure is allowed to end the current function early. That is strictly
more than Python gives you, for less ceremony than Go's manual check.
