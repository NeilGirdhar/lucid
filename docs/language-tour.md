# Language tour

A quick walkthrough of the core mechanics, each one runnable through the
reference implementation (see [Getting started](getting-started.md)).

## Two user-defined types

Lucid separates obligations and reusable behavior from owned state into two
constructs, not three — a `trait` mixes required methods with reusable
default ones freely, so no separate interface keyword is needed for the
obligation-only case:

1. **`trait`**: A member with no body is a required obligation; a member
   with a body is reusable default behavior. Traits own no fields.
2. **`class`**: Concrete types with owned fields and constructors. Classes
   permit at most one class parent (single inheritance).

```python
trait Greeter:
    def greet(self) -> str

trait Friendly:
    def greet(self) -> str:
        return "Hello, " + self.name + "!"

class User(Friendly):
    name: str

u = User("Alice")
print(u.greet())  # "Hello, Alice!"
```

## Mutability views (`T`, `&T`, `!T`)

Lucid tracks mutability in the type system:

* `T` — Mutable reference (can mutate fields).
* `&T` — Read-only view (cannot mutate through this reference; underlying
  data might mutate).
* `!T` — Deeply immutable object (frozen; cannot mutate anywhere).

```python
class Account:
    balance: int

acc = Account(100)
acc.balance = 150  # OK: mutable

# Deep freeze transitions to !Account
frozen = freeze(acc)
# frozen.balance = 200  # Type error: cannot mutate attribute on frozen object !Account
```

## Multiple dispatch binary operators

Lucid uses symmetric multiple dispatch for binary operations rather than
Python's asymmetric reflected methods (`__radd__`):

```python
class Vector2D:
    x: float
    y: float

dispatch def +(a: Vector2D, b: Vector2D) -> Vector2D:
    return Vector2D(a.x + b.x, a.y + b.y)

dispatch def *(v: Vector2D, s: float) -> Vector2D:
    return Vector2D(v.x * s, v.y * s)

v1 = Vector2D(1.0, 2.0)
v2 = Vector2D(3.0, 4.0)
v3 = v1 + v2
print(v3.x, v3.y)  # 4.0 6.0
```

## Recoverable errors with `?`

Recoverable errors are ordinary values. Propagate them early with `?`:

```python
class NotFoundError:
    message: str

def find_user(id: int):
    if id == 42:
        return "Alice"
    return NotFoundError("user not found")

def get_welcome_message(id: int):
    name = find_user(id)?
    return "Welcome, " + name + "!"

print(get_welcome_message(42))  # "Welcome, Alice!"
print(get_welcome_message(99))  # NotFoundError("user not found")
```

## Anonymous records

Replace arbitrary dicts or tuples with typed anonymous records:

```python
point = record { x: 10, y: 20 }
print(point.x + point.y)  # 30
```

## Built-in primitives and collections

Lucid provides standard primitives:

* `range(stop)`, `range(start, stop)`, `range(start, stop, step)`
* `len(x)` on strings, lists, dicts, sets, records
* `min()`, `max()`, `sum()`
* `x in collection` and `x not in collection`
* Comprehensions for lists, dicts, and sets:

  ```python
  evens = [x for x in range(10) if x % 2 == 0]
  squared_map = {str(x): x * x for x in range(4)}
  unique_chars = {c for c in "abracadabra"}
  ```
* File I/O:

  ```python
  write_file("greeting.txt", "Hello Lucid")
  content = read_file("greeting.txt")
  ```

## Multi-file modules and standard library

Import sibling files or standard modules:

```python
# math_demo.lucid
import math
from math import sqrt, pi

print(sqrt(25.0))  # 5.0
print(pi > 3.0)     # true
```

Relative module imports:

```python
# utils.lucid
export def square(x: int) -> int:
    return x * x

# main.lucid
from .utils import square
print(square(8))  # 64
```
