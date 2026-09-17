# Member metadata

Python documents a class's attributes by convention only — a `#:`
comment above one, a convention Sphinx invented and only Sphinx checks,
or a hand-written "Attributes:" section inside the class's own
docstring, disconnected from the attribute list the same way
[a function's own per-parameter documentation already is](metadata.md).
Rename the attribute and either form silently goes stale.

A field's docstring and metadata dict are the same `;` block as
anywhere else, standing on the line right after the field:

```python
class Layer:
    weights: Array
    ; "the layer's learnable weight matrix"

    activation: str = "relu"
    ; "the nonlinearity applied after the affine transform"
    ; {"static": true}
```
`fields()` (see [Field reflection with `fields`](construction.md#field-reflection-with-fields))
reads both back at runtime, alongside the field's own name and current
value:

```python
layer = Layer(weights, "relu")
list(fields(layer))[1]  # (name="activation", value="relu",
                         #  doc="the nonlinearity applied after the affine transform",
                         #  metadata={"static": true})
```
Renaming `activation` moves its metadata block with it, since the two
are parsed together as one statement — there is no separate copy of the
name for the docstring to fall out of sync with.

## Trait members

A class's own `fields()` walks only its stored fields, but a trait can
require any kind of member — methods, getters, setters, factories — and
its own `fields()` walks all of them, docstring and metadata included,
the same way [Body or no body](traits.md#body-or-no-body) already
treats every kind of member alike:

```python
trait Sized:
    def __len__(self: ~Self) -> int
    ; "the number of elements this container holds"

list(fields(type Sized))[0]  # (name="__len__", obligation=true,
                              #  doc="the number of elements this container holds",
                              #  metadata={:})
```
