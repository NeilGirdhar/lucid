# Construction

Building an instance and introspecting a class's own fields are two
related jobs, both centered on the factory as where they meet — neither
about what kinds of members a class body can declare, which
[Classes](classes.md) covers on its own. A factory field can also be
filled with a value captured from its own call site, a mechanism that
applies to any function, not just factories, covered in
[Call-site captured values](call-site-captured-values.md).

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
