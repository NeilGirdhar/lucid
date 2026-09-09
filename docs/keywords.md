# Keywords

Apart from [Builtins](builtins.md) and behaviors
explicitly replaced in this specification, Lucid preserves Python's
ordinary keyword vocabulary.

## Preserved Python keywords

The preserved Python keywords, linked where this specification gives
one new or narrowed meaning:
[`and`](type-operations.md#intersection-types),
[`as`](match.md#exhaustive-pattern-matching), [`assert`](assert.md),
`async`, `await`,
[`break`](for-and-while.md#if_broken-loop-clauses),
[`class`](types.md#what-class-means-in-lucid),
[`continue`](for-and-while.md#unspecified-simple-statements),
[`def`](calls.md#anonymous-functions),
[`del`](names.md#del-ends-a-names-lifetime-early),
[`elif`](match.md#exhaustive-pattern-matching),
[`else`](for-and-while.md#no-loop-else), [`except`](exceptions.md),
[`finally`](exceptions.md), [`for`](for-and-while.md),
[`from`](import.md), [`if`](if.md), [`import`](import.md), `in`,
[`is`](identity-checks.md), [`not`](type-operations.md#negation-types),
[`or`](type-operations.md#intersection-types),
[`pass`](for-and-while.md#unspecified-simple-statements),
[`raise`](exceptions.md),
[`return`](match.md#exhaustive-pattern-matching),
[`try`](exceptions.md), [`while`](for-and-while.md),
[`with`](with.md), and
[`yield`](context-managers.md#one-modifier-one-shape).

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

Lucid discards `global` and `nonlocal`, Python's scope-rebinding
declarations — see the `No global` and `No nonlocal` rules in
[Scope](scope.md).

Lucid discards `lambda`. It is redundant, not a sign that anonymous
functions are gone: an unnamed `def` is one (see
[Anonymous functions](calls.md)), needing no keyword of its own.

