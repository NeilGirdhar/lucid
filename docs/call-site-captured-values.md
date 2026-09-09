# Call-site captured values

A function body can read something about its own call expression —
where it was written, what its result is being assigned to — through
two intrinsic classmethods, `SourceLocation.caller()` and
`VarName.from_assignment()`. Both resolve fresh at each call site
instead of once, at definition time, the way an ordinary default
would.

Both are intrinsics, not ordinary classmethods: the checker recognizes
these two exact names on these two exact built-in types and gives them
special treatment. Defining a same-named classmethod on some other
class gets none of it — this is not a general naming convention to
extend, only these two calls on these two types.

## How the substitution works

Rust's closest equivalent, `#[track_caller]`, needs to be an
explicit, per-function opt-in, because an ordinary Rust function can
sit arbitrarily deep in a call chain, and the attribute has to say how
many layers of wrapping to see through before reaching the real
caller. Lucid needs no such opt-in, because every call — `log(...)`,
`Traceback()`, `Sentinel()` — is already a specific, recognizable call
expression the checker sees as a distinct syntactic form, regardless of
what kind of function it calls. Compiling that call already means
compiling that exact expression, with its module, its line, and (when
the call is a bare assignment right-hand side) its target name sitting
right there in the syntax tree. The compiler passes those along as
hidden arguments to the call, the same trick `#[track_caller]` uses —
just applied unconditionally to every call, since the call site itself
is already the distinguishing mark an explicit attribute would
otherwise have to supply.

## Caller-captured source locations

Any parameter typed `SourceLocation` can default to
`SourceLocation.caller()`, meaning "the module and line of this call
expression":

```python
def log(message: str, where: SourceLocation = SourceLocation.caller()) -> none:
    print(f"[{where}] {message}")

log("starting up")   # [config.lcd:12] starting up
```
A factory field fills the same way, through `construct`:

```python
class Traceback:
    location: SourceLocation

    factory __init__(cls):
        return construct(SourceLocation.caller())

Traceback()   # Traceback at config.lcd:12
```
This is the same category of mechanism as Rust's `file!()`/`line!()`
and `#[track_caller]`: the substitution is fixed and entirely local to
the one call expression it appears in — understanding what it does
requires reading nothing else in the codebase, unlike attribute hooks or
behavior inherited from elsewhere in a class hierarchy.
`SourceLocation.caller()` is always available: every call happens
somewhere, so there is always a module and line to substitute.

## Name-captured identifiers

A parameter or factory field typed `VarName` can be filled with
`VarName.from_assignment()`, meaning "the identifier this call's
result is being assigned to." It resolves the same way
`SourceLocation.caller()` does, fresh at each call site, but it is
not always available: a call is only the direct right-hand side of a
simple assignment sometimes, not always — it might instead be an
argument, a return value, or a target of some other shape, such as a
tuple or chained assignment. Those have no single identifier to
substitute, and using `VarName.from_assignment()` there is a
compile-time error at that call site: the checker already knows, from
the call's syntax alone, whether a name exists to capture, the same
way it already knows whether `SourceLocation.caller()` fills a
`SourceLocation`-typed slot.

```python
class Sentinel:
    name: VarName

    factory __init__(cls):
        return construct(VarName.from_assignment())

    def __repr__(self: ~Self) -> str:
        return f"<Sentinel {self.name}>"

missing = Sentinel()   # <Sentinel missing>
log(Sentinel())        # error: Sentinel() has no named assignment target
```
Every `Sentinel()` gets the name it was assigned to, with nothing to
write twice or let drift out of sync — the same category of mechanism as
Python's `__set_name__`, just restricted to exactly the one call
expression it substitutes into instead of a class body.
