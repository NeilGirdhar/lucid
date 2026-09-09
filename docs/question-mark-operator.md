# The `?` operator

Matching every fallible call by hand does not scale. Go's version of this
problem is `if err != nil { return nil, err }` repeated after nearly
every call — legible, but the boilerplate outweighs the logic it surrounds.
`?` is sugar for the common case: propagate the error case unchanged, or
unwrap to the value and keep going, over the return-type-based errors
[Results](results.md) covers.

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
