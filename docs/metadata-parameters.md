# Parameter metadata

[Metadata blocks](metadata.md) opens with the reason this page exists:
Python has no real convention for documenting one parameter at all.
Google, NumPy, and reST docstring styles each restate a function's
parameter list as prose inside the docstring, parsed by a different
tool's own regular expressions, checked against nothing:

```python
# Python
def transfer(amount, from_account, to_account):
    """Move money between two accounts.

    Args:
        amount: the amount to move, in the account's currency.
        from_account: the source account number.
        to_account: the destination account number.
    """
```
Rename `amount` to `quantity` and the `Args:` line above still says
`amount` — nothing connects the two names, so nothing catches the
drift. Lucid attaches a parameter's docstring directly inside the
parameter list, as part of the same statement the parameter itself is
declared in:

```python
def transfer(
    amount: float; "the amount to move, in the account's currency",
    from_account: str,
    to_account: str,
) -> none:
    ...
```
Renaming `amount` moves its docstring along with it — there is no
second copy of the name anywhere to fall out of sync.

## A parameter's metadata dict

The same block can instead carry a data dict, exactly like a field's
(see [Member metadata](metadata-members.md)) — for example, marking a
parameter that a JAX-style pytree flattener should treat as static
auxiliary data rather than a traced value:

```python
def transfer(amount: float, from_account: str; {"static": true}) -> none:
    ...
```
## Reading it back

A function is not one of `fields()`'s receivers — see [Field
reflection with `fields`](construction.md#field-reflection-with-fields)
— so a parameter's docstring and metadata are read through `Callable`'s
own [`.parameters`](types.md#reading-a-signatures-own-parameters)
property instead:

```python
transfer.parameters[1]  # (name="from_account", kind=PositionalOrKeyword,
                         #  type_=str, default=Sentinel(), doc=none,
                         #  metadata={"static": true})
```
