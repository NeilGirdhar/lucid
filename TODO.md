# Forward references

Reordering `zensical.toml`'s top-level nav groups (Types, Expressions,
Statements, Argument bundles, Functions, Containers, Traits and
classes, ...) left a number of cross-group links pointing at content
the reader hasn't reached yet. `AGENTS.md` already accepts "a handful
of forward pointers... where two topics genuinely reference each other
for comparison" — most entries below are exactly that, and probably
don't need fixing. The ones under **High priority** are different: they
lean on notation or a mechanism from a not-yet-read group as load-bearing
content, not just a "see also."

Fixing any of these means either moving the target earlier, moving the
source later, or rewriting the passage so it no longer depends on the
unread material.

## High priority — load-bearing, not just a citation

### Argument bundles → Traits and classes

`Arguments`/`Parameters`/`Gather`/`Spread` type `pargs` as an
**Anonymous class** and lean on **Construction** as part of explaining
their own mechanism, not as an aside — but Traits and classes now
reads after this group.

- `docs/arguments.md:4` — Anonymous class
- `docs/gather.md:18` — Anonymous class
- `docs/parameters.md:13` — Class inheritance
- `docs/parameters.md:15,27,37,44,58` — Anonymous class (5 uses)
- `docs/spread.md:7,47` — Factory construction

### Expressions → Traits and classes

- `docs/identity-checks.md:31` — the non-overlapping-type-test example
  depends on "a constructor call infers as final A" from Construction.

## Medium priority — illustrative examples, not the core mechanism being explained

### Argument bundles → Functions (Decorators)

`gather.md`/`parameters.md`/`spread.md` use Decorators as a worked
example of `***` in practice; the mechanism itself doesn't need it.

- `docs/gather.md:18`
- `docs/parameters.md:22`
- `docs/spread.md:15,86,101`

### Functions → Traits and classes

- `docs/context-managers.md:46` — Modern type specification (Overview)
- `docs/context-managers.md:53` — Explicit overrides (Traits)
- `docs/dispatch.md:148` — Higher-kinded traits

### Statements → Traits and classes

- `docs/for-and-while.md:122` — No del on fields (Classes)
- `docs/match.md:54` — sealed class (Class inheritance)

## Low priority — light "see also" citations, likely fine as intentional forward pointers

### Statements → Functions

- `docs/exceptions.md:4`, `docs/for-and-while.md:9` — Results
- `docs/for-and-while.md:45` — Name-captured identifiers
- `docs/match.md:117` — Dispatch beyond operators
- `docs/with.md:6` — Context managers

### Types → Traits and classes

- `docs/generics.md:215` — implement (Traits)
- `docs/mutability.md:193,200` — Factory construction, Field reflection
- `docs/mutability.md:233` — Explicit overrides (Traits)
- `docs/types.md:64` — Modern type specification (Overview)
- `docs/types.md:95` — Anonymous class
- `docs/types.md:255` — Constructor calls infer as final

### Types → Functions

- `docs/generics.md:212` — Multiple dispatch
- `docs/match-types.md:40` — Promotion
- `docs/numeric-types.md:223` — Results

### Types → Statements / Expressions

- `docs/match-types.md:8` — Exhaustive pattern matching
- `docs/types.md:268` — Identity and instance checks

### Expressions → Statements / Functions / Traits and classes

- `docs/calls.md:64,167` — Exhaustive pattern matching
- `docs/identity-checks.md:41` — Multiple dispatch
- `docs/question-mark-operator.md:8` — Results

### Names → Statements

- `docs/destructuring.md:29`, `docs/names.md:11` — Destructuring with match

### Traits and classes → Projects

- `docs/class-inheritance.md:67,115`, `docs/type-specification.md:12`
  — Module-private names

### Miscellaneous single occurrences

- `docs/collections.md:168` — Modern type specification (Overview)
- `docs/decorators.md:28` — No __module__ or __qualname__ (project.yaml)
- `docs/for-and-while.md:123` — No __delitem__ (Indexing)
- `docs/with.md:11` — Rejected features

## Not included

`docs/principles.md` (Main ideas) is excluded — it's a survey of ideas
elaborated everywhere else in the spec, so it forward-references
almost every other document by design.
