# Keywords

Apart from [Builtins](builtins.md#lowercase-constants) and behaviors
explicitly replaced in this specification, Lucid preserves Python's
ordinary keyword vocabulary.

## Preserved Python keywords

The preserved Python keywords are `and`, `as`, `assert`, `async`,
`await`, `break`, `class`, `continue`, `def`, `del`, `elif`, `else`,
`except`, `finally`, `for`, `from`, `if`, `import`, `in`, `is`, `not`,
`or`, `pass`, `raise`, `return`, `try`, `while`, `with`, and `yield`.

## New Lucid keywords

Lucid adds keywords for construction, destructuring, class member
kinds, closing off rebinding or further class inheritance or
overriding, explicit overrides, declining generated behavior,
guaranteed-cleanup context managers, abstraction, dispatch, external
trait implementation, type expressions, existential quantification,
exhaustive pattern matching, and elision. Listed in the order each is
first defined: [`final`](names.md#final-local-variables),
[`_`](names.md#black-hole-assignment-with-_), [`let`](destructuring.md),
[`type`](types.md#type-aliases), [`trust`](casting.md),
[`without`](mutability.md#equality-ordering-and-hashing),
[`any`](generics.md#existential-types),
[`skip`](calls.md#skip-in-calls),
[`if_broken`](for-and-while.md#if_broken-loop-clauses),
[`match`](match.md), [`case`](match.md),
[`dispatch`](dispatch.md#dispatch-beyond-operators),
[`contextmanager`](context-managers.md#one-modifier-one-shape),
[`trait`](traits.md), [`override`](traits.md#explicit-overrides),
[`implement`](traits.md#implementing-a-trait-after-the-fact),
[`getter`](classes.md#no-property), [`setter`](classes.md#no-property),
[`classvar`](class-members.md#class-member-variables),
[`classmethod`](class-members.md#explicit-class-methods),
[`sealed`](class-inheritance.md#sealed-classes),
[`factory`](construction.md#factory-construction), and
[`construct`](construction.md#factory-construction).

## Discarded Python keywords

Lucid discards `global`, `nonlocal`, and `lambda`. `global` and
`nonlocal` are Python's scope-rebinding declarations — see the `No
global` and `No nonlocal` rules in [Scope](scope.md). `else` is not
discarded as a keyword. Lucid removes loop `else` clauses,
but `else` remains available for the Python-like constructs that still use
it. `lambda` is discarded because it is redundant, not because anonymous
functions are gone: an unnamed `def` is one (see
[Anonymous functions](calls.md)), needing no keyword of its own.

