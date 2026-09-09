# Scope

## No scope-rebinding declarations

A statement's meaning should not depend on some other statement elsewhere in
the same function. Python's `global`/`nonlocal` violate this: whether
`x = value` binds a new local or rebinds an enclosing name depends on a
declaration that can sit anywhere in the function body, so an assignment can
retroactively change what an *earlier, unrelated-looking* read meant:

```python
counter = 0

def increment():
    print(counter)    # UnboundLocalError
    counter += 1      # this line is why
```
Lucid removes the declaration rather than live with that coupling: assignment
is always local, independent of anything else in the function. Sharing
mutable state across scopes instead goes through an explicit object, so the
sharing is visible at the object's declaration and at each call site that
touches it, rather than hidden behind a keyword elsewhere in the function.

### No `global`

Python uses `global` to let assignment inside a function rebind a module
binding:

```python
counter = 0

def next_id() -> int:
    global counter
    counter += 1
    return counter
```
Lucid represents shared module state with an explicit mutable object:

```python
counter = Cell(0)

def next_id() -> int:
    counter.value += 1
    return counter.value
```
### No `nonlocal`

Python uses `nonlocal` to let assignment inside an inner function rebind a
name from an enclosing function:

```python
def make_counter():
    count = 0

    def next():
        nonlocal count
        count += 1
        return count

    return next
```
Closure state uses the same pattern:

```python
def make_counter() -> () -> int:
    count = Cell(0)

    def next() -> int:
        count.value += 1
        return count.value

    return next
```
