# Statement metadata

A metadata block can attach to any statement, not just a `def` or
`class` — the same `;` syntax [Metadata blocks](metadata.md) introduces
works on a plain assignment, an `if`, a `for`, or any other statement a
suite can contain.

## Suppressing one check on one statement

Python's `# noqa` and `# type: ignore` comments only work as a trailing
comment on the exact reported line: move the statement to a new line
during a refactor and either the check comes back silently or the
comment now silently suppresses whatever new statement lands under it.
Neither is checked against a real list of checks, so a typo'd code
(`# noqa: E50` for `E501`) suppresses nothing and reports nothing.

`ignore` is a metadata block like any other, parsed as part of the
statement's own grammar rather than floating text beside it:

```python
x: int = 5; ignore: unused_variable
```
The same block can stand alone on the line right after the statement it
covers, attaching the same way:

```python
x: int = 5
; ignore: unused_variable
```
A misspelled check name — `ignore: unused_varible` — is the linter's
own error, the same way an unknown field name already is elsewhere, not
a silently ineffective comment.

## Documenting a compound statement

When a metadata block is the first line of a suite a statement just
opened, it attaches to that statement's own header rather than to
anything inside the suite:

```python
def transfer(amount: float, from_account: str, to_account: str) -> none:
    ; "Move money between two accounts."
    ...
```
The same statement can use the triple-quoted shorthand instead — see
[Triple-quoted strings are docstring shorthand](metadata.md#triple-quoted-strings-are-docstring-shorthand).

## No runtime reflection

Unlike a class member or a function parameter, an ordinary statement's
metadata has no property that reads it back at runtime: `fields()`
walks a class's, trait's, or module's members (see [Member
metadata](metadata-members.md)), and `Callable.parameters` walks a
function's own parameters (see [Parameter
metadata](metadata-parameters.md)), but a local assignment or a `for`
loop is neither. Its metadata exists for the reader and the linter
alone.
