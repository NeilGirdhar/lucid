# Class inheritance

*Class inheritance* stays explicit and does not use Python's metaclass or MRO
customization hooks.

## One class parent

*Class inheritance* is limited to one parent: a class may have at most one
class parent. A class header can combine one class parent with any
number of traits.

```python
class FileLogger(LoggerBase, Closeable, Timestamped):
    path: str
```
## Final classes

`final` applies to a class the same way it applies to a field: a
relationship is fixed permanently, no matter who is asking. On a field, that
relationship is the binding; on a class, it is class inheritance. A
`final` class may not be used as anyone's class parent:

```python
final class Point:
    x: float
    y: float

class Point3D(Point):  # error: Point is final
    z: float
```
This lets an author close off a class specifically because a subclass could
violate an invariant the implementation depends on, without that decision
touching anything about fields or ordinary variable bindings — the same
`final` keyword, applied one level up.

## Sealed classes

`sealed` sits between the default and `final` on the same axis: an
ordinary class may be subclassed from anywhere, a `final` class cannot
be subclassed at all, and a `sealed` class may be subclassed only by
classes declared in the same file:

```python
sealed class Shape:
    def area(self: ~Self) -> float

class Circle(Shape):
    radius: float
    def area(self: ~Self) -> float:
        return pi * self.radius ** 2

class Rectangle(Shape):
    w: float
    h: float
    def area(self: ~Self) -> float:
        return self.w * self.h
```
A file elsewhere in the project cannot add a fourth direct subclass of
`Shape` — verified the same way `final` already verifies that no
subclass exists anywhere, just narrower in scope. The restriction applies
only to `Shape`'s direct subclasses: `Circle` and `Rectangle` stay
ordinarily subclassable elsewhere unless they are separately marked
`sealed` or `final`, since `sealed` only has to pin down `Shape`'s
own variant set, not freeze every subtree beneath it.

A source file is already the unit module-private names are scoped to
([Module-private names](modules.md)), so there is no separate notion
of "module" for this to disagree with, the ambiguity languages with
multi-file modules have to resolve one way or another — the file sealing
restricts to is the same file every other module boundary in Lucid
already uses.

Sealing gives [Exhaustive pattern matching](match.md) a second
source of closed types, alongside recursive union aliases: a `match`
over `Shape` with a case for every direct subclass needs no `case _:`,
because the checker can see the complete set the same way it already can
for a union.

## No private-name mangling

Python rewrites a name such as `self.__a` to include the defining class's
name. This name mangling helps prevent accidental collisions between members
introduced by different classes, especially in multiple-inheritance
hierarchies.

A class can have at most one class parent. The restriction is not about the
parent being non-abstract — it is that a class, unlike a trait, owns
stored data, and combining stored data from more than one parent
is exactly what produces the collisions above. Traits do not own stored
state, and conflicting trait defaults must be resolved explicitly. Because class shape is declared and inherited member
collisions can therefore be reported directly, Lucid does not need automatic
name mangling. Declare and use `a` as `self.a`; `self.__a` is not a
special spelling for private state.

## Private members

A member named with a leading `_` is private to the class: readable
and writable from the class's own methods, and from nowhere else, not
even other files in the same project.

```python
class Cache:
    _entries: dict[str, float]

    def get(self: ~Self, key: str) -> float | none:
        return self._entries.get(key)

cache = Cache({:})
cache._entries  # error: _entries is private to Cache
```
Python's single leading underscore is a convention nothing enforces —
`cache._entries` already works fine, the same as any other attribute.
Lucid checks it: the name alone is the complete, checked declaration,
the same way it already is for a module-private definition at the top
level of a file (see [Module-private names](modules.md)).

## No metaclasses

Metaclasses are not part of Lucid. A restricted
`__init_subclass__(cls, **options)` may remain for validation and registration
only, not for class rewriting.

## No `__prepare__`

`__prepare__` is not part of Lucid because metaclass machinery is absent.

## No `__mro_entries__`

`__mro_entries__` is not part of Lucid. Lucid does not allow MRO rewriting.

## No programmable type checks

`__instancecheck__` and `__subclasscheck__` are not part of Lucid. Type
relationships are not programmable through indirect hooks.

[Construction](construction.md) covers how these classes get built — the
factory model that fills in the fields this group has been declaring all
along.
