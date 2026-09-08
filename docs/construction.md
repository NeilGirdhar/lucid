# Construction

Building an instance, introspecting a class's own fields, and filling a
factory field with a value captured from the call site are three
related jobs, all centered on the factory as where they meet — none of
them about what kinds of members a class body can declare, which
[Classes](classes.md) covers on its own.

## Factory construction

Python splits construction across `__new__`, `__init__`,
dataclass-generated initializers, `__post_init__`, `InitVar`, and field
options such as `init=False` or `kw_only`. Lucid has one construction model:
factories return fully constructed objects through the factory-only
`construct` keyword.

Factories receive `cls`, but they construct the exact class where they are
defined. Calling a class calls its `__init__` factory. Calling a named factory
uses the factory name on the class.

```python
class Point:
    x: float
    y: float

    factory __init__(cls, x: float, y: float):
        return construct(x, y)

    factory origin(cls):
        return construct(0.0, 0.0)

p = Point(1.0, 2.0)
origin = Point.origin()
```
If `__init__` is unspecified, Lucid generates the obvious field-based
constructor. This class:

```python
class Point:
    x: float
    y: float
```
gets this default `__init__` factory:

```python
factory __init__(cls, x: float, y: float):
    return construct(x, y)
```
`construct` is a keyword, not an ordinary function. It can only appear inside
a factory. At runtime, a `construct` expression creates an instance of the
exact class whose factory is running, assigns the supplied values to that
class's declared fields in field order, and returns the fully initialized
object.

Factories are not inherited.

Every class has a generated `replace` factory. It works like Python's
`__replace__` protocol: given an existing instance and any changed field
values, it constructs a new instance of the same exact class with unchanged
fields copied from the original object.

```python
class Point:
    x: float
    y: float

p = Point(1.0, 2.0)
q = Point.replace(p, y=3.0)
```
## Field reflection with `fields`

`replace` and the default constructor both already have to walk a
class's fields generically. `fields` exposes that same walk directly,
the way Python's `dataclasses.fields` does, dispatched on whether it is
given an instance or the class itself:

```python
def dispatch fields[T](obj: T) -> Iterable[(name: str, value: object, doc: str | none, metadata: dict[str, object])]:
    ...

def dispatch fields[T](cls: type[T]) -> Iterable[(name: str, doc: str | none, metadata: dict[str, object])]:
    ...
```
Both yield fields in declaration order. The instance form pairs each
field's name with its current value; the class form has no instance to
read a value from, so it yields only names. Both carry `doc` and
`metadata` from [Field docstrings and metadata](classes.md), `none`
and `{:}` respectively when a field declares neither.

```python
class Config:
    name: str:
        "the user's display name"

c = Config("Ada")
list(fields(c))[0]      # (name="name", value="Ada", doc="the user's display name", metadata={:})
list(fields(Config))[0]  # (name="name", doc="the user's display name", metadata={:})
```
## Call-site captured values

A factory body can read something about its own call expression —
where it was written, what its result is being assigned to — through
two intrinsic classmethods, `SourceLocation.caller()` and
`VarName.from_assignment()`. Both resolve fresh at each call site
instead of once, at definition time, the way an ordinary default
would. This is not a member-declaration mechanic either — it belongs
here because a factory is where these values are actually consumed.

Both are intrinsics, not ordinary classmethods: the checker recognizes
these two exact names on these two exact built-in types and gives them
special treatment. Defining a same-named classmethod on some other
class gets none of it — this is not a general naming convention to
extend, only these two calls on these two types.

### How the substitution works

Rust's closest equivalent, `#[track_caller]`, needs to be an
explicit, per-function opt-in, because an ordinary Rust function can
sit arbitrarily deep in a call chain, and the attribute has to say how
many layers of wrapping to see through before reaching the real
caller. Lucid needs no such opt-in, because the scope here is already
narrower and already syntactically marked: both intrinsics are legal
only inside a factory body, and every factory call — `Traceback()`,
`Sentinel()` — is a specific, recognizable call expression the
checker already sees as a distinct syntactic form. Compiling that call
already means compiling that exact expression, with its module, its
line, and (when the call is a bare assignment right-hand side) its
target name sitting right there in the syntax tree. The compiler
passes those along as hidden arguments to the factory invocation, the
same trick `#[track_caller]` uses — just applied unconditionally to
every factory, since being a factory is already the distinguishing
mark an explicit attribute would otherwise have to supply.

### Caller-captured source locations

A factory field typed `SourceLocation` can be filled with
`SourceLocation.caller()`, meaning "the module and line of this call
expression":

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

### Name-captured identifiers

A factory field typed `VarName` can be filled with
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
`SourceLocation`-typed field.

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

