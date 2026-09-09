# Exceptions

`raise`, `try`, `except`, and `finally` work exactly as they do in
Python, narrowed to the other kind of failure [Results](results.md)
doesn't cover: a broken invariant, a bug, something that should never
happen — Rust's `panic!`, not Rust's `Result`. They are not for
everyday, expected outcomes the way Python's `StopIteration`-driven
iteration or its `KeyError`-then-catch idiom use them.

```python
try:
    validate(config)
except ConfigError:
    log.error("invalid config, aborting")
    raise
finally:
    connection.close()
```

`raise` stays unchecked — not declared in a function's signature, not
enforced by the checker. That is deliberate, not an oversight: the
visibility Java wanted from checked exceptions is already delivered by
recoverable errors being ordinary return types. Checking the broken-
invariant case too would just be ceremony around something no caller is
meant to routinely handle in the first place.
