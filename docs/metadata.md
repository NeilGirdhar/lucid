# Metadata blocks

## Three unchecked conventions, doing one job

Python has three separate, informal ways to attach something to a piece
of code that only a *tool* — a reader, an IDE, a linter — needs to see,
and none of them are checked.

A **docstring** is a bare string literal, conventionally the first
statement of a module, class, or function. Nothing enforces that it's
actually first, that there's only one, or that it says anything true.

**Per-parameter documentation** has no convention of its own at all.
Projects invent one inside the docstring's own text instead — Google
style, NumPy style, reST field lists — three incompatible dialects for
the same job, each parsed by a different tool's own regular expressions,
none of them checked against the function's actual parameters. Rename a
parameter and every one of these silently goes stale.

A **suppression comment** — `# type: ignore`, `# noqa` — silences one
specific checker complaint. It shares nothing with the docstring
convention even though both exist to tell a tool something about the
line they sit next to.

Lucid replaces all three with one construct, checked the same way
everywhere it appears: a *metadata block*, introduced by `;`.

## One marker, three payloads

A metadata block is `;` followed by exactly one of three things, read
as a real expression — a genuine string or dict literal, fully
tokenized, not scanned as raw text — so a comma or `#` inside a nested
string or dict doesn't end the block early:

```python
; "the amount to move, in the account's currency"   # a docstring
; {"static": true}                                   # metadata
; ignore: unused_import                              # a linter directive
```

A bare string is a docstring. A bare dict is metadata — arbitrary,
tool-facing data, the way `{"static": true}` might tell a JAX-style
pytree flattener to treat a field as static auxiliary data instead of a
traced array leaf; Lucid never looks inside it. `ignore` is the
one word a metadata block itself recognizes, naming a check the linter
should not report here; `unused_import` is not checked by the compiler
at all — it's validated against the linter's own registry of known
checks, the same way a misspelled field name is the linter's problem,
not the parser's.

A `#` comment may follow any of the three, carrying the human reason —
the same job a comment already does everywhere else:

```python
; ignore: unused_import  # kept for its side effect on import
```

## Several directives at once

`;` also separates one directive from the next, so a block can carry
more than one entry, either on one line:

```python
x: int = 5; ignore: unused_variable; ignore: shadowed_name
```

or as several standalone lines, each its own metadata block:

```python
x: int = 5
; ignore: unused_variable
; ignore: shadowed_name
```

A binding may have at most one docstring and at most one metadata dict;
a second one is a checker error, the same way a duplicate field or a
duplicate import already is. `ignore` has no such limit — there's
nothing wrong with suppressing several unrelated checks on one line.

## Where a block attaches

Trailing on the same line, a metadata block attaches to whatever
precedes it on that line — the same "attaches to what's immediately to
its left" reading a trailing `# comment` already has, just checked
instead of merely conventional. Standing alone on its own line, it
attaches to the statement immediately above it, at the same
indentation; if nothing at that indentation precedes it — it's the
first line of a suite a compound statement just opened — it attaches to
that statement's own header instead, since a metadata block is legal
content for a suite the same way an ordinary statement is.

Three contexts put this rule to work: [Statement
metadata](metadata-statements.md) covers an ordinary statement or a
compound one's own header, [Member metadata](metadata-members.md)
covers a field inside a class or trait, and [Parameter
metadata](metadata-parameters.md) covers one parameter inside a
signature.

## Triple-quoted strings are docstring shorthand

A triple-quoted string standing alone on its own line is shorthand for
a metadata block carrying it as a docstring, [always dedented](strings.md#triple-quoted-strings-are-always-dedented)
the way every triple-quoted string already is:

```python
def transfer(amount: float, from_account: str, to_account: str) -> none:
    """Move money between two accounts."""
    ...
```
reads exactly as

```python
def transfer(amount: float, from_account: str, to_account: str) -> none:
    ; "Move money between two accounts."
    ...
```
and follows the same attachment rule as any other standalone metadata
block — the statement above it, or the enclosing suite's own header
when it's that suite's first line, the same way `name: str` followed
by a triple-quoted line attaches to the field.

This is the one place a bare string literal still means something.
Any other bare string statement — single- or double-quoted, or a
triple-quoted string sharing a line with other code — is an ordinary,
pointless expression statement, and the checker rejects it the same
way it already rejects other code that looks like a well-known
convention but silently isn't, such as the parenthesized-`assert`
requirement in [Assert](assert.md) or the names in
[Removed builtins](removed-builtins.md). A bare dict has no such
shorthand either — `{...}` alone is still just a discarded value, not
metadata, because accidentally leaving one behind is a real mistake
worth catching, not a convention worth recognizing.

## Reading metadata blocks back

Two properties read a docstring or metadata dict back at runtime,
depending on what carries it: `fields()` ([Field reflection with
`fields`](construction.md#field-reflection-with-fields)) covers a
field, a class, a trait, or a module — see [Member
metadata](metadata-members.md). A function's own parameters go through
`Callable`'s own [`.parameters`](types.md#reading-a-signatures-own-parameters)
property instead, since a function is not one of `fields()`'s receivers
— see [Parameter metadata](metadata-parameters.md). `ignore` is
different from both: it exists only for the linter, never appears in
either one's output, and has no runtime meaning at all.
