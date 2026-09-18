# Self-hosting Lucid

Lucid's compiler (`compiler/`) is written in Rust. This directory is the
start of writing pieces of it in Lucid itself — a normal milestone for a
new language's toolchain, not a replacement effort. See the design pillars
in [../docs/principles.md](../docs/principles.md) for the language, and
[../docs/architecture.md](../docs/architecture.md) for the Rust
implementation.

## Status

The target is full parity with the Rust implementation (`compiler/`) —
lexer, parser, checker, both codegen backends, then bootstrapping. That
is a large, multi-stage undertaking; this records real progress toward
it, not a claim that it's close to done. All five pieces now exist in
some form — lexer, parser, a tree-walking interpreter, a checker, and a
C codegen — and `compile_demo.lucid` proves the checker+codegen half
actually produces correct native executables, not just well-formed C;
none of the five is close to feature parity with its Rust counterpart.

- `lexer.lucid` — a lexer, tokenizing Lucid source into the same token
  kinds `compiler/crates/lucid-syntax/src/lexer.rs` produces. Runs under
  the reference interpreter (`lucid run`) and successfully tokenizes its
  own source (`lexer_demo.lucid` proves this — see below). Not yet at
  feature parity with the Rust lexer; deferred work is listed at the top
  of the file itself.
- `lexer_demo.lucid` — tokenizes a small sample program, then reads and
  tokenizes `lexer.lucid`'s own source. The second part is the actual
  self-hosting proof point: Lucid source a human wrote, run by the
  reference interpreter, correctly lexing Lucid source — including
  itself.
- `ast.lucid` — the AST every later piece (parser, checker, codegen) will
  build and consume, mirroring `compiler/crates/lucid-syntax/src/ast.rs`
  node for node: `Expr`, `Stmt`, `Pattern`, `TypeExpr`, `ClassMember`,
  `TraitMember`, `LiteralValue`, and their supporting structs, as sealed
  class hierarchies. Not yet covered: `MetadataPayload`/`MetadataDirective`
  (the `;`-introduced docstring/meta/ignore blocks) — `Stmt.Metadata` and
  `ClassMember.Metadata` hold a placeholder `text: str` for now. The
  module-root struct is `ParsedModule`, not `Module` as in `ast.rs` —
  `Module` is a reserved builtin runtime concept (see "Design notes"
  below).
- `ast_demo.lucid` — builds a small expression tree by hand (`1 + (x * 2)`)
  and walks it with an exhaustive `match` over the sealed `Expr`
  hierarchy imported from `ast.lucid`, proving the AST is constructible
  and matchable under the interpreter, not just type-checkable.
- `parser.lucid` — a recursive-descent parser turning `lexer.lucid`'s
  token stream into `ast.lucid`'s AST, mirroring
  `compiler/crates/lucid-syntax/src/parser.rs`'s structure: the same
  precedence chain for expressions, one method per statement/pattern/type
  form. Not at feature parity; deferred work (decorators, generics,
  trait/implement/module blocks, comprehensions, error recovery, and
  more) is listed at the top of the file itself. Runs under the reference
  interpreter and successfully parses `lexer.lucid`'s own source
  (`parser_demo.lucid` proves this — see below), `ast.lucid`'s (~15s), and
  its own (~99s, being much larger and almost entirely one big `class
  Parser:` body).
- `parser_demo.lucid` — parses a small sample program, then reads and
  parses `lexer.lucid`'s own source. The second part is the self-hosting
  proof point for this piece: Lucid source a human wrote, run by the
  reference interpreter, correctly parsing real Lucid source into a
  well-formed AST.
- `interpreter.lucid` — a tree-walking evaluator over `ast.lucid`'s
  `Expr`/`Stmt` nodes: no checker or codegen pass in between, just direct
  execution, the way `compiler/crates/lucid-runtime` runs a checked
  program. A genuine subset, not feature parity — traits, dispatch,
  exceptions, sealed-class inheritance (a subclass doesn't inherit a
  parent's fields/methods), set values, slicing, imports,
  comprehensions, generators, async, context managers, closures over
  anything but a call's own parameters, and keyword/variadic/default
  parameters are all left for later, listed at the top of the file
  itself — but what's covered (recursion, `while`/`for`, `if`/`elif`/
  `else`, classes with fields and methods (including `self` mutation
  that persists across calls), `Construct` calls, attribute get/set,
  list/dict/str indexing, list/dict values and literals, `match` over a
  class name or `none`, `?` as a statement's own value expression, the
  arithmetic/comparison/logical operators, `print`/`len`/`str`/`int`/
  `float`, and `list.append`/`.pop`/`dict.get`) is enough to run real,
  substantial programs — including `lexer.lucid` itself.
- `interpreter_demo.lucid` — takes three small hand-written programs as
  plain-text Lucid source (a recursive `fib`; an iterative `factorial`
  over a list; a fourth program covering globals, `elif`/`else`,
  `break`/`continue`, `not`/`and`/`or`, unary `-`, and floats) and runs
  each one, as text, through `lexer.lucid` then `parser.lucid` then
  `interpreter.lucid`, printing their real output — plus a program that's
  *supposed* to fail (an undefined name inside a list literal), proving
  errors surface correctly too, not just the happy path.
- `self_hosting_demo.lucid` — loaded, parsed, and interpreted proof that
  the pieces above work together, in two parts. Part 1 loads
  `lexer.lucid`'s own source — its `Lexer`, `Token`, and `LexError`
  classes, its `tokenize()`/`new_lexer()` functions — into the
  self-hosted parser and interpreter, calls its own `tokenize()` on a
  sample program, and checks the result token-by-token against calling
  the *host-run* `tokenize()` (imported directly, executed by the
  reference interpreter) on the same sample — a real differential test,
  not eyeballing a token list, that would catch an off-by-one in string
  indexing or a wrong `self.line`/`column` mutation. Part 2 goes one
  level deeper: loads `ast.lucid`, `lexer.lucid`, and `parser.lucid`
  together and calls parser.lucid's own `parse()` — the self-hosted
  interpreter running the self-hosted parser, which itself calls the
  self-hosted lexer, all as interpreted code. Lucid, running Lucid,
  running Lucid.
- `checker.lucid` — a static checker over `ast.lucid`'s `Expr`/`Stmt`
  nodes: no execution, just name resolution (every variable read and
  function call resolves to something in scope) and call-arity checking,
  over the `int`/`bool`-only subset `codegen.lucid` compiles. Not feature
  parity with `compiler/crates/lucid-checker` (~20,000 lines covering
  generics, traits, variance, exhaustiveness, and far more) — no real
  type inference beyond "is this `int`/`bool`", no flow-sensitive
  definite-assignment analysis — but real, useful checking: duplicate
  function names, undefined names, wrong-arity calls, and unsupported
  types are all rejected with a specific message.
- `checker_demo.lucid` — runs `checker.lucid` against one clean program
  and one program for each kind of error it catches, printing what it
  found.
- `codegen.lucid` — a C code generator for the same subset
  `checker.lucid` accepts: every function becomes a C function (a Lucid
  function named `main` becomes C's own `int main(void)`), every local
  variable is a C `long`, arithmetic/comparison/logical operators map
  straight to their C equivalents, and `print(x)` becomes
  `printf("%ld\n", x)`. Lucid has no subprocess/exec builtin, so codegen
  stops at emitting C text — invoking a system C compiler on it is
  necessarily a driver step outside Lucid, the same role
  `compiler/crates/lucid-codegen` itself plays (it also just shells out
  to `gcc`, from Rust rather than from the Lucid it compiles).
- `compile_demo.lucid` — **the milestone this directory has been
  building toward: an actual compiler, written in Lucid, producing a
  real native executable.** Lexes, parses, checks, and generates C for
  three real Lucid programs (a recursive `fib`, an iterative
  `factorial`, a `gcd`/`classify` pair exercising `%`, `and`/`or`, and
  comparisons) — entirely through `lexer.lucid`, `parser.lucid`,
  `checker.lucid`, and `codegen.lucid` — and a fourth program that's
  supposed to fail checking, proving a real error stops codegen instead
  of emitting broken C. Each valid program's C output is written to
  `self-host/compile_demo_<name>.c` (gitignored — regenerated by running
  the demo); `gcc -std=c11 -o <binary> self-host/compile_demo_<name>.c`
  compiles it, and running the resulting binary produces the exact
  correct output (`compiler/crates/lucid-cli/tests/spec_tests.rs`'s
  `self_hosted_compile_demo_produces_correct_native_binaries` test is
  that external driver, checked in). Lucid, compiling Lucid, to a native
  executable.

Run them:

```
lucid run self-host/lexer_demo.lucid
lucid run self-host/ast_demo.lucid
lucid run self-host/parser_demo.lucid
lucid run self-host/interpreter_demo.lucid
lucid run self-host/self_hosting_demo.lucid
lucid run self-host/checker_demo.lucid
lucid run self-host/compile_demo.lucid
gcc -std=c11 -o /tmp/fib self-host/compile_demo_fib.c && /tmp/fib
```

Self-tokenizing takes on the order of 20-30 seconds under the debug
interpreter build — `lexer.lucid` builds its output strings one character
at a time (`text = text + c`), which is quadratic in a debug build with no
inlining. Worth revisiting once there's more here to justify it (either a
StringBuilder-style pattern in the language, or building lists of
characters and joining once), but it works, which is what "start on"
called for. Self-parsing (tokenize + parse) `lexer.lucid`'s ~500 lines
takes on the order of 30 seconds for the same reason, one layer up;
`self_hosting_demo.lucid`'s Part 1 (parse `lexer.lucid`, then interpret
it, then have that interpreted code tokenize a small sample and compare
it against the host-run lexer) takes on the order of 35 seconds, one
layer up again; Part 2 (parse and interpret `ast.lucid`, `lexer.lucid`,
and `parser.lucid` together, then have that interpreted code parse a
small sample) takes on the order of 3 minutes, stacking every layer
above on top of each other.

None compile under `lucid run --native` yet — see "Native codegen
gaps" below. All run correctly under the reference interpreter, which is
the primary target for this work; native is a stretch goal.

## Cross-file imports actually work now

Building this out of more than one file surfaced a real, significant gap:
`from <module> import <name>` bound every imported name — a class, a
function, a plain variable — to `Any`, unconditionally, in the checker.
A class imported this way couldn't even be constructed
(`Circle(2.0)` failed with `unknown enclosing class 'Circle'`), and a
wrong-typed call to an imported function type-checked anyway. This
blocked splitting the compiler into files that import each other's
classes at all, so it's fixed now, three commits: `TypeChecker::
merge_import_from` in `lucid-checker`, wiring it into `lucid-db`'s
`project_diagnostics` (what `lucid check`/`lucid run` actually consult),
and removing three redundant single-file checks in `lucid-cli`'s native
entry points that were still hitting the old `Any` fallback independently
even after the first two fixes landed. See those commits' messages for
the full story.

## `?` silently no-op'd inside a nested expression (fixed)

The most serious bug found writing any of this: `?` failing inside a
function-call argument, a construct argument, a list/set/dict literal
element, or a binary/unary operand didn't propagate — it silently handed
the raw error object to whatever was evaluating that subexpression, as if
it were the ordinary success value, and execution just continued.

`items.append(parse_one()?)` is the shape that surfaced it:
`Expr::Propagate`'s runtime handler does the real work of turning a
failing call into a `Value::Return` sentinel meant to unwind the
*enclosing function* — but nothing at the call site that evaluates
`items.append(...)`'s arguments checked for that sentinel before handing
the value to `append`. `append` received the error object as an ordinary
argument, the statement "succeeded", and whatever loop was calling
`parse_one()` kept going. When the loop's own exit condition depended on
progress that same failed call was supposed to make — exactly the shape
of `parser.lucid`'s `while not self.check("DEDENT"): body.append(self.
parse_class_member()?)` — the result wasn't a wrong answer, it was an
infinite loop: `self.pos` never advances on a failed parse, so the next
iteration re-parses the identical failing token forever.

This is what was actually behind the "self-parsing `ast.lucid` hangs"
symptom investigated (and initially misdiagnosed as a performance
problem) earlier in this file's history — `ast.lucid` has a field
literally named `module` (`class Import(Stmt): module: str`), and
`module` is Lucid's own reserved keyword ([Design notes](#design-notes)
already covers `out` and capitalized-first-letter names as two other
reserved-name surprises), so parsing it always failed — and every one of
those failures hit this bug instead of surfacing as a normal parse error.
Fixing the runtime bug turned the symptom from "hangs forever" into
"fails in under a second with a clear message", which is what led to
finding and fixing the actual field-naming collision
(`module` → `module_name`) in minutes instead of chasing a performance
ghost. Both `parser.lucid` and `interpreter.lucid` lean on this exact
`x.append(y()?)` / `Ctor(y()?)` pattern throughout, so this one runtime
fix is likely what makes self-parsing `ast.lucid` and `parser.lucid`
itself (not just `lexer.lucid`) possible at all — see the `parser.lucid`
entry above.

Fixed in `lucid-runtime` by adding an `eval_operand!` macro — evaluate,
check for `Value::Return`, re-propagate if found, otherwise use the value
— and applying it everywhere a subexpression's value is used for
something other than being returned directly: call arguments (plain,
spread, and gather-spread), construct arguments, list/set/dict literal
elements, and binary/unary operands. This was found and fixed at the
specific sites this work actually exercises, not via an exhaustive audit
of every expression kind `eval_expr` handles (a ~10,000-line match) — a
real audit of the rest is worth doing separately.

A related but distinct gap, *not* fixed here: `?`'s error-recognition
itself (both in the checker and, separately, in this runtime) picks the
error variant(s) out of a union by a literal name-suffix check
(`ends_with("Error")`), not by any structural signal — a custom error
type not named `*Error` (e.g. `MyErr`) is invisible to `?` even once the
`Value::Return`-propagation bug above is fixed, and silently behaves as
a success value instead. Every error type in this codebase already
follows the `*Error` convention, so it hasn't blocked anything here, but
it's a real, separate inconsistency between the checker's (partial, with
a same-file fallback) and the runtime's (none) handling of the same rule.
The fix belongs in the checker, not here: `Expr::Propagate`'s runtime
handler only ever sees one concrete `Value` with one concrete class
name — it has no access to the enclosing function's declared return type
or the full static union the checker reasons about, so it has no way to
tell "this is the one non-`Error`-suffixed type that happens to be the
error" from "this is the one non-`Error`-suffixed type that happens to be
the success value" the way the checker's fallback does. The checker is
the right place to close this gap — for instance, by rejecting a `?`
whose error variant doesn't end in `Error` outright, so the convention
this whole codebase already follows becomes a real, enforced rule instead
of an implicit one a future `MyErr`-named type could still silently fall
through.

## Native codegen gaps

Found while getting `ast_demo.lucid` to compile under `--native`; not
fixed, since both are open-ended enough to deserve their own scoped
work rather than a rushed fix bundled in here:

- **A field or parameter named the same as a C keyword breaks native
  codegen.** `ast.lucid` originally had `TypeParam.default`, matching
  `ast.rs`'s own field name — C reserves `default` (a `switch` label),
  and codegen emits struct field and constructor parameter names
  directly from the Lucid identifier with no escaping, so the generated
  C failed to compile (`LucidVal default;` and worse). Worked around
  here by renaming to `default_value`; the real fix belongs in
  `lucid-codegen`, escaping any Lucid identifier that collides with a C
  keyword wherever it's emitted as a raw C name (struct fields,
  constructor parameters, at minimum) — not attempted here since finding
  every such emission site in a 21,000-line file is its own project.
- **Attribute access on a match-narrowed variable doesn't inherit the
  narrowed type.** `match e as result: case Binary: return result.op`
  compiles `result.op` (statically a `str`, once narrowed to `Binary`)
  through the fully dynamic `lucid_dynamic_attr` accessor, typed as
  `LucidVal`/`double`, not `const char*` — so concatenating it into a
  string (`"(" + result.op + " " + ...`) miscompiles as numeric
  addition, the same failure shape as the string-index bug fixed
  earlier in `infer_expr_type`, but for match-narrowed attribute access
  instead of indexing. Fixing this needs codegen to track a match arm's
  narrowed type per-binding, a bigger feature than a local fix to one
  `infer_expr_type` arm.
- Also still true from the lexer's notes: native codegen has no
  structural knowledge of the builtin exception hierarchy (`Exception`,
  `ParseError`, `ValueError`, ...) the way the checker and interpreter
  both do, so `match ... case ParseError:` fails to compile.

## Design notes

- **Token kinds are strings, not a class hierarchy.** The reference
  lexer's `TokenKind` has ~90 variants, most of them payload-free
  keywords and operators. A one-subclass-per-variant sealed hierarchy —
  the pattern the docs use for a closed set of cases with real behavior
  per case — would mean ~90 essentially-empty classes here. `Token.kind`
  is instead a plain string tag (`"DEF"`, `"PLUS"`, `"IDENT"`, ...), and
  `Token.text` carries the spelling for the kinds that have one
  (identifiers, numbers, strings). Revisit if a later piece (the parser)
  wants to `match` on kind and would benefit from exhaustiveness
  checking.
- **Errors are `Result`s, not `raise`.** A malformed source file is
  exactly the "expected, recoverable" failure
  [Results](../docs/results.md) covers, not a broken invariant — so
  `next_token`/`tokenize` return `Token | LexError` /
  `list[Token] | LexError` and propagate with `?`, the same way
  `read_file` does.
- **`out` is a reserved word.** It's Lucid's variance-annotation keyword
  ([Generics](../docs/generics.md)), not available as an ordinary
  identifier — a first draft of `lexer_demo.lucid` used it for a local
  variable and got a parse error.
- **A module-level variable needs a lowercase name to be visible inside a
  function.** `KEYWORDS: dict[str, str] = {...}` (capitalized, matching
  the constant-naming convention this repo's Rust code and most
  Python-family languages use) silently failed to bind — not just inside
  functions, even a `print` on the very next top-level line couldn't see
  it — while `keywords: dict[str, str] = {...}` (lowercase) works fine.
  Lucid reserves a capitalized leading letter for type/class names
  ([Names](../docs/names.md)), and that convention turns out to be
  load-bearing here in a way that fails silently rather than with a clear
  error. Worth a follow-up: either enforce it as a real, diagnosed rule,
  or stop treating capitalization as significant for validity.
- **`Literal` is a reserved type constructor, not an available class
  name.** `ast.rs`'s `Expr::Literal` variant became `LiteralExpr` here,
  not `Literal` — `Literal[value]` is Lucid's own literal-type syntax
  ([Literal types](../docs/types.md#literal-types)), so naming a class
  `Literal` fails with "unknown enclosing class" the moment it's
  constructed (the checker resolves the construct call against the
  builtin `Literal[...]`, never registers a same-named user class in
  `env.classes`).
- **`Module` and `ParseError` are reserved, not available class names.**
  `Module` is a builtin runtime module-value concept distinct from an
  AST's own module-root node — a `class Module:` construct fails with
  "construct for 'Module' has no field named ...", since the checker
  resolves it against the builtin, never registering the user class. Same
  failure shape for `ParseError`: the checker hardcodes it as a builtin
  `Exception` subclass with a single `message: str` field, so a
  same-named user class with a different shape fails constructor checks
  with a confusing arity/field mismatch rather than a naming collision
  error. `ast.lucid`'s module-root struct is `ParsedModule`;
  `parser.lucid`'s parse-error type is `ParserError`.
- **A user error type's name has to end in `Error` for `?` to recognize
  it.** The `?` operator picks out which union member(s) to propagate by
  a literal name-suffix check (`name.ends_with("Error")`), not by
  subtyping against `Exception` or any other structural signal — so
  renaming `parser.lucid`'s error type away from the reserved
  `ParseError` to something that doesn't end in `Error` (a first attempt
  used `ParseFailure`) silently broke every `?` whose union had more than
  one non-`Error`-suffixed member: `?` only falls back to "first member
  is the success type, the rest are errors" when *no* member ends in
  `Error` at all, so a union like `ParsedModule | ParseFailure | LexError`
  still found `LexError` by suffix and left `ParseFailure` in the success
  side, alongside `ParsedModule` — and callers saw a type error naming
  that two-member union rather than anything mentioning `?`
  itself. Naming it `ParserError` (distinct from the builtin
  `ParseError`, and ending in `Error`) fixed it.
- **A `sealed class`'s fields and methods live in one body, subclasses
  in others.** A subclass inherits the parent's fields the ordinary way
  (declare `span: Span` once, on `sealed class Expr:`, and every
  `class Literal(Expr): value: LiteralValue` gets it as its first
  constructor argument) — there's no separate `extend ClassName:` block
  for adding methods after the fact; a first draft of `lexer.lucid`
  assumed one and got a parse error. Fields and every method both belong
  directly in the one `class Foo:` body.
- **`Point(3.0, 4.0)`-shaped syntax is `Expr::Construct`, not
  `Expr::Call`.** Lucid parses any call to a capitalized identifier as a
  `Construct` expression, not a plain function `Call` — relevant if a
  later piece of this needs to distinguish "calling a function" from
  "constructing a class" while walking `ast.lucid`'s `Expr` hierarchy:
  they're already two different node types, not one shape you have to
  tell apart by convention.
- **A variable's declared type sticks for the rest of the function, even
  across sibling `if` blocks.** `parser.lucid` originally reused the name
  `value` in two independent `if` branches of the same function — one
  declaring `value: Expr | none = none`, a later sibling branch plainly
  reassigning `value = self.parse_expr()?`. The second assignment isn't a
  fresh binding: Lucid tracks one declared type per name for the whole
  function, from wherever it's first declared, so the later branch's
  value still typed as `Expr | none` even though only non-`none` values
  ever reach it — rejecting a perfectly typed `Expr` against a plain
  `Expr` parameter downstream. Fixed by giving the second branch its own
  name (`assign_value`) instead of reusing `value`.
- **Reassigning a `match` statement's own scrutinee inside an arm is a
  read-only-variable error.** `match expr as result: case Ident: expr =
  Construct(...)` fails with "cannot reassign to read-only or immutable
  variable 'expr'" — the checker treats a plain-variable scrutinee as
  immutable for the duration of the match, presumably so the narrowing
  each arm relies on (`result`'s type per case) can't be invalidated
  mid-match. Fixed by matching on a copy (`call_target = expr`) and
  reassigning `expr` from that instead.
- **A `match` needs a real wildcard to close out a sealed hierarchy —
  naming the base class doesn't count.** `case Expr:` or `case TypeExpr:`
  written as an intended catch-all still leaves the match
  non-exhaustive: the checker requires either every subclass covered by
  name or an actual `case _:`. Same shape as the `sealed class`/`extend`
  mistake above — an assumption carried over from a different language's
  pattern matching, not Lucid's.
- **...but naming the sealed base *is* a valid, narrowing pattern when
  it's one arm of a union, not the whole hierarchy being discriminated.**
  `interpreter.lucid` matches a `field: Expr | none` with `case Expr: ...
  case none: ...` to tell "there's a value" from "there isn't" — that's
  a different situation from the point above (matching `Expr`'s own
  subclasses exhaustively) and works exactly as hoped: `case Expr:`
  matches any concrete subclass instance and narrows the binding to
  `Expr` inside that arm, the same way `case int:` / `case LexError:`
  already did in `lexer_demo.lucid`'s `int | LexError` result.
- **A bare generic type name in a pattern doesn't narrow — it needs its
  type arguments spelled out.** `match x as v: case list: ...` type-checks
  but `v` inside that arm keeps `x`'s original (wider) type, rejecting a
  later call that needs the element type — `case list[Stmt]:` narrows
  correctly. Only came up because `If.else_branch: list[Stmt] | none`
  needed the narrowed arm passed to a `list[Stmt]`-typed parameter.
- **A no-op class member isn't a field named `"pass"` — it's its own
  node.** `parser.lucid` first turned `class Foo: pass` into a
  `FieldMember` with a made-up field literally named `"pass"`, since
  that satisfied the checker (any field name type-checks) and the bug
  stayed invisible until something actually *constructed* a `Foo`:
  `ast.lucid` is full of `class Break(Stmt): pass`-shaped classes, and
  `Break(span)` under `interpreter.lucid` would have silently bound
  `fields["pass"] = span` and looked like it worked. `ast.lucid` already
  has the right node for this, `PassMember(ClassMember)` — parsing
  `pass` inside a class body should return that, not fabricate a field.
- **`x = value` is always `Stmt::Assignment`, never `Stmt::VarDef`, even
  the first time `x` appears.** `VarDef` is only for the annotated form,
  `x: T = value` (or `let x = value`) — the parser doesn't look ahead to
  decide "is this name new" the way a hand-written codegen might assume.
  `codegen.lucid` first generated a C declaration (`long x = ...;`) only
  from `VarDef` nodes and plain `x = ...;` from `Assignment` nodes,
  matching how real Lucid programs are actually written (bare assignment
  for a variable's first use, not `x: int = ...`) produced C referencing
  an undeclared `x`. Fixed by having codegen collect every VarDef/
  Assignment target name across a whole function body first (including
  inside nested `if`/`while` blocks, since Lucid gives a name the same
  function-wide visibility either way, unlike C's own per-block
  scoping) and hoist one `long` declaration per name to the top of the
  generated C function; every `VarDef`/`Assignment` node then just emits
  a plain `x = ...;`.
