# Construction

Building an instance and introspecting a class's own fields are two
related jobs, both centered on the factory as where they meet — neither
about what kinds of members a class body can declare, which
[Class members](class-members.md) covers on its own. A factory field can also be
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

Every class has a generated `replace` method. It works like Python's
`__replace__` protocol: given any changed field values as keyword
arguments, it builds a new instance of the same exact class with
unchanged fields copied from `self`, and hands back an ordinary,
mutable value — regardless of which view called it:

```python
class Point:
    x: float
    y: float

p: !Point = freeze(Point(1.0, 2.0))
q: Point = p.replace(y=3.0)   # a fresh, ordinary Point, not !Point
```
`replace` only ever reads `self` to build the new object; it never
writes to it, so it takes `self: ~Self`, callable through a mutable,
read-only, or frozen view alike. This does not strain
[freezing being deep](mutability.md#freezing-is-deep): that rule
governs what a view exposes about the *original* object's own
storage, and `replace` never exposes that storage — it constructs a
brand new instance, as free to be ordinarily mutable as any other
freshly constructed value.

## Constructor calls infer as `final`

A call naming the class it constructs can only ever produce an
instance of exactly that class — building a subclass instead needs its
own constructor call, `B()`, not `A()`. Lucid infers this precisely:
`A()`'s type is [`final A`](types.md#final-types), not plain `A`:

```python
class A: ...

a = A()   # final A
```
This is the constructor counterpart of literal inference: `1` infers
as `Literal[1]` and widens to `int` wherever a declaration governs
it, and `final A` widens to `A` in exactly the same places:

```python
class B(A): ...

class C:
    x: A = A()

def g(c: C):
    c.x = B()   # ok — C.x's declared type is A, not final A

items: list[A] = [A()]   # list[A], not list[final A]
```

## Field reflection with `fields`

`replace` and the default constructor both already have to walk a
class's fields generically. `fields` exposes that same walk directly,
the way Python's `dataclasses.fields` does, dispatched on what it's
given — an instance, a class, a trait, or a module:

```python
def dispatch fields[T](obj: T) -> Iterable[(name: str, value: object, doc: str | none, metadata: dict[str, object])]:
    ...

def dispatch fields[T](cls: class[T]) -> Iterable[(name: str, doc: str | none, metadata: dict[str, object])]:
    ...

def dispatch fields(trait: type Trait) -> Iterable[(name: str, obligation: bool, doc: str | none, metadata: dict[str, object])]:
    ...

def dispatch fields(mod: Module) -> Iterable[(name: str, doc: str | none)]:
    ...
```
The instance and class forms yield fields in declaration order — the
instance form pairs each field's name with its current value; the
class form has no instance to read a value from, so it yields only
names. Both carry `doc` and
`metadata` from [Field docstrings and metadata](class-members.md), `none`
and `{:}` respectively when a field declares neither.

```python
class Config:
    name: str:
        "the user's display name"

c = Config("Ada")
list(fields(c))[0]      # (name="name", value="Ada", doc="the user's display name", metadata={:})
list(fields(Config))[0]  # (name="name", doc="the user's display name", metadata={:})
```
The trait form walks a trait's own declared members — fields,
getters, setters, methods, classmethods, and factories alike —
reporting whether each one is a bodyless obligation or a default with
a body ([Body or no body](traits.md#body-or-no-body)), the same
distinction the checker already uses to decide whether a class using
the trait still has something left to implement. `type Trait` reifies
the trait the same way [Reifying a type
expression](types.md#reifying-a-type-expression) already reifies any
other type expression, since a trait is not itself a callable value
the way a class is:

```python
list(fields(type Sized))[0]  # (name="__len__", obligation=True, doc=none, metadata={:})
```
The module form walks a module's own top-level, visible definitions —
functions, classes, traits, and module-level bindings — the structured
replacement for Python's `dir()`: names in declaration order, each with
its own docstring, rather than an unordered list of strings with
nothing else attached.
