# Getting Started with Lucid

Lucid is a statically-typed, expressive language design combining Python's syntax elegance with compile-time type safety, Julia-style multiple dispatch, explicit mutability views, and ergonomic result-based error handling.

This guide walks through building the Lucid compiler, using the interactive REPL, executing scripts, and exploring the language.

---

## 1. Installation and Setup

### Prerequisites

- **Rust toolchain** (edition 2024 compatible, Rust 1.85+ recommended).

### Building from Source

Clone the repository and build the workspace:

```bash
git clone https://github.com/npow/lucid.git
cd lucid

# Build release binary
cargo build --release

# The compiled binary is located at:
# ./target/release/lucid
```

Optionally, add `./target/release` to your `PATH`, or alias it:

```bash
alias lucid="$(pwd)/target/release/lucid"
```

---

## 2. Using the Lucid CLI

The `lucid` command-line tool provides everything needed to run, typecheck, evaluate, and experiment with Lucid code.

```
Lucid Language Compiler & Runtime

USAGE:
    lucid [COMMAND] [OPTIONS]

COMMANDS:
    repl             Start interactive REPL (default when no arguments)
    run <file>       Parse, typecheck, and evaluate a Lucid source file
    check <file>     Parse and typecheck a Lucid source file
    eval <code>      Evaluate a Lucid code snippet string
    test-spec [dir]  Extract and validate code snippets from RST specification docs
    help             Display this help message
    version          Show version information
```

### Interactive REPL

Launch the REPL simply by running `lucid` or `cargo run -p lucid-cli`:

```bash
$ lucid
Lucid 0.1.0 interactive REPL
Type :help for assistance, :exit or :quit to leave.

>>> 2 + 2
4
>>> words = "lucid is clean and expressive".split()
>>> [w.upper() for w in words if len(w) > 4]
["CLEAN", "EXPRESSIVE"]
>>> def add(a: int, b: int) -> int:
...     return a + b
... 
>>> add(10, 32)
42
>>> :exit
```

**REPL Commands:**
- `:help` — Display REPL tips and available commands.
- `:vars` — List currently defined variables and values in the environment.
- `:clear` — Clear the screen.
- `:reset` — Reset the environment and type checker.
- `:exit` (or `:quit`) — Exit the REPL.

### Running Files

Execute a Lucid source file (`.lucid`):

```bash
lucid run examples/hello.lucid
```

### Typechecking Files

Verify types without executing runtime code:

```bash
lucid check examples/hello.lucid
```

### Evaluating One-Liners

```bash
lucid eval "print([x * x for x in range(6)])"
# Output: [0, 1, 4, 9, 16, 25]
```

---

## 3. Language Tour

### Three User-Defined Types

Lucid cleanly separates obligations, stateless reuse, and owned state into three distinct constructs:

1. **`interface`**: Abstract obligations with no state and no method bodies.
2. **`trait`**: Reusable behavior with method bodies, but no owned fields.
3. **`class`**: Concrete types with owned fields and constructors. Classes permit at most one class parent (single inheritance).

```python
interface Greeter:
    def greet(self) -> str

trait Friendly:
    def greet(self) -> str:
        return "Hello, " + self.name + "!"

class User(Friendly):
    name: str

u = User("Alice")
print(u.greet())  # "Hello, Alice!"
```

### Mutability Views (`T`, `&T`, `!T`)

Lucid tracks mutability in the type system:
- `T` — Mutable reference (can mutate fields).
- `&T` — Read-only view (cannot mutate through this reference; underlying data might mutate).
- `!T` — Deeply immutable object (frozen; cannot mutate anywhere).

```python
class Account:
    balance: int

acc = Account(100)
acc.balance = 150  # OK: mutable

# Deep freeze transitions to !Account
frozen = freeze(acc)
# frozen.balance = 200  # Type error: cannot mutate attribute on frozen object !Account
```

### Multiple Dispatch Binary Operators

Lucid uses symmetric multiple dispatch for binary operations rather than Python's asymmetric reflected methods (`__radd__`):

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

### Recoverable Errors with `?`

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

### Anonymous Records

Replace arbitrary dicts or tuples with typed anonymous records:

```python
point = record { x: 10, y: 20 }
print(point.x + point.y)  # 30
```

### Built-in Primitives and Collections

Lucid provides standard primitives:
- `range(stop)`, `range(start, stop)`, `range(start, stop, step)`
- `len(x)` on strings, lists, dicts, sets, records
- `min()`, `max()`, `sum()`
- `x in collection` and `x not in collection`
- Comprehensions for lists, dicts, and sets:
  ```python
  evens = [x for x in range(10) if x % 2 == 0]
  squared_map = {str(x): x * x for x in range(4)}
  unique_chars = {c for c in "abracadabra"}
  ```
- File I/O:
  ```python
  write_file("greeting.txt", "Hello Lucid")
  content = read_file("greeting.txt")
  ```

### Multi-File Modules & Standard Library

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

---

## 4. Editor Support (VS Code)

Syntax highlighting and language configurations are included in `editors/vscode`:

```bash
# Link the extension into VS Code
ln -s "$(pwd)/editors/vscode" ~/.vscode/extensions/lucid
```

Then reload VS Code. All `.lucid` files will have full syntax coloring, auto-closing brackets, and intelligent indentation.

---

## 5. Running Specification Tests

Verify the entire language specification test suite:

```bash
# Run all unit and integration tests
cargo test --workspace

# Validate documentation code snippets against the parser and checker
cargo run -p lucid-cli -- test-spec
```
