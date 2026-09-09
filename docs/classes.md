# Classes

Classes define concrete state, construction, and identity. A class can
use any number of traits and use *class inheritance* to extend at
most one other class, but stored fields and construction belong to the
class.

```python
class Point:
    x: float
    y: float

    factory __init__(cls, x: float, y: float):
        return construct(x, y)

    getter distance_from_origin(self) -> float:
        return sqrt(self.x ** 2 + self.y ** 2)
```
Factories construct fully initialized instances of the exact class where the
factory is defined. Construction is not split across allocation, mutation, and
post-initialization hooks — see [Construction](construction.md) for the
full factory model.

Classes own data and concrete behavior. Traits declare obligations, provide
reusable method bodies, or both. Conflicting trait defaults are resolved
explicitly by the class.

## Classes have visible state

Classes should make their stored state visible. Lucid takes the transparent
field-first style of dataclasses and makes it the normal object model rather
than a library convention layered over dynamic objects. Stored object state is
declared in the class body, and attribute access is structural and visible in
the class body, not programmable through dynamic lookup hooks.

Python instances usually have an open-ended `__dict__` unless a class uses
`__slots__`, dataclasses with options, extension types, or custom attribute
hooks. Lucid makes fixed shape the default: stored fields are declared directly
in the class body.

```python
class Point:
    x: float
    y: float
```
## No undeclared fields

Assigning an undeclared field is an error. Lucid does not give instances an
implicit `__dict__` for arbitrary attributes. If a class needs dynamic keyed
data, declare that storage explicitly. Adding or removing class members after
definition is also not part of Lucid; class shape is closed. Assigning
`obj.__class__` is not part of Lucid because object shape is fixed.

```python
p = Point(1.0, 2.0)
p.z = 3.0  # error: z is not a declared field
```
```python
class Record:
    fields: dict[str, object]

    def get(self, name: str) -> object | none:
        return self.fields.get(name)

    def set(self, name: str, value: object):
        self.fields[name] = value
```
## No `del` on fields

A field is part of its class's fixed shape, not an optional slot present
only when set — deleting one would leave an object whose layout no
longer matches its own class, the same violation [Classes have visible state](#classes-have-visible-state) already rules out for adding one. `del obj.field` is a
compile-time error for every declared field, checked the same way
assigning an undeclared one already is:

```python
p = Point(1.0, 2.0)
del p.x  # error: fields are fixed, not deletable
```
## No descriptors

Python descriptors can make attribute access programmable from many places.
Lucid does not include descriptors. Attribute behavior is visible through
fields, methods, getters, setters, class methods, factories, and class member
variables.

## No `property`

Python's `property` is descriptor-based. Lucid uses explicit getter and setter
member syntax instead.

```python
class Circle:
    radius: float

    getter area(self) -> float:
        return pi * self.radius ** 2

    setter area(self, value: float):
        self.radius = sqrt(value / pi)
```
## No `__getattr__`

Lucid does not include `__getattr__` fallback lookup. Missing attributes are
errors instead of calls into dynamic lookup code.

## No `__getattribute__`

Lucid does not include `__getattribute__`. Attribute reads use visible members
from the class body and cannot be globally intercepted.

## No `__setattr__`

Lucid does not include `__setattr__`. Attribute assignment targets a declared
field or an explicit setter.

## No `__del__`

Lucid implements the bracketing pattern — acquire, use, release — with
[context managers](context-managers.md), not RAII-style cleanup tied
to an object's lifetime. Python's `__del__` is non-deterministic — the
garbage collector decides when, or whether, to run it — so a file
descriptor, socket, or lock can leak for the rest of the process. Lucid
has no `__del__`.

A Swift-style `deinit`, run when an object's last reference drops,
looks like a fix, but "last reference" is only well-defined once the
language tracks reference counts or ownership for every value, and
copying anything that owns a resource has to be restricted so two
owners can't both release it. A context manager gets the same
determinism from a lexical scope instead: cleanup runs at the
`with`-block's boundary, visible at the call site, with none of that
machinery needed.

An unnamed class shape follows the same declared-not-dynamic rule as a
named one's own fields — [Anonymous class](anonymous-class.md) covers it
next.
