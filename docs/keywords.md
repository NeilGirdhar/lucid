# Keyword reference

## Lowercase constants

Lucid spells its boolean and null constants in lowercase:

```text
false none true
```
## Preserved Python keywords

Apart from the lowercase constants and behaviors explicitly replaced in this
specification, Lucid preserves Python's ordinary keyword vocabulary. The
preserved Python keywords are:

```text
and as assert async await break class continue def del elif else except
finally for from if import in is not or pass raise return try while
with yield
```
## New Lucid keywords

Lucid adds keywords for construction, destructuring, class member
kinds, closing off rebinding or further class inheritance or
overriding, explicit overrides, declining generated behavior,
guaranteed-cleanup context managers, abstraction, dispatch, external
trait implementation, type expressions, existential quantification,
exhaustive pattern matching, and elision:

```text
classmethod classvar factory construct
let
getter setter
final sealed override
without
contextmanager
trait
dispatch
implement
type
any
trust
match case
if_broken
skip
_
```
An earlier draft added `export` here, marking a public definition at
its own declaration. That needed a keyword at every public definition
plus a project-wide manifest repeating the same paths a second time.
Visibility is now decided by the name alone — see
[Module-private names](modules.md) — and the manifest is the only
place the externally visible surface is declared, in
[Public API](project-configuration.md) — so no keyword is needed at
the definition site at all.

An earlier draft also added `caller` and `from_var_name` here, two
bare reserved words for two rare, unrelated call-site captures. Both
are now ordinary-looking intrinsic classmethod calls instead,
`SourceLocation.caller()` and `VarName.from_assignment()` (see
[Call-site captured values](construction.md)) — recognized by name
on their two built-in types the way Rust's `Location::caller()` is
recognized without needing a keyword either, so two reserved words
became zero.

An earlier draft also had `interface` alongside `trait` — a
separate kind whose members were always bodyless, obligations only,
never reusable behavior. Whether a trait member is an obligation or a
default already follows from something checked either way, the
presence or absence of a body, so a second keyword was only ever
saying which bucket the whole block belonged to, not adding a
distinction the checker needed. Merging removed the one case that
distinction actually cost something: a capability with a small
required core and one or two obvious defaults on top no longer needs
two cross-referencing declarations for what is really one idea (see
[Traits](traits.md)).

## Discarded Python keywords

Lucid discards Python's scope-rebinding declarations — see the `No global`
and `No nonlocal` rules in
[Binding, destructuring, and scope](names.md):

```text
global nonlocal
lambda
```
`else` is not discarded as a keyword. Lucid removes loop `else` clauses,
but `else` remains available for the Python-like constructs that still use
it. `lambda` is discarded because it is redundant, not because anonymous
functions are gone: an unnamed `def` is one (see
[Anonymous functions](calls.md)), needing no keyword of its own.

