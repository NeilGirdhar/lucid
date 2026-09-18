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
C codegen — and `compile_demo.lucid` compiles a real, unmodified program
from `examples/` (`vectors.lucid`, using classes and multiple dispatch)
to a native executable whose output matches the reference interpreter's
own output exactly: Lucid, compiling Lucid, to a native binary — for the
subset covered so far, not yet for the whole language. None of the five
pieces is close to feature parity with its Rust counterpart.

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
- `types.lucid` — the type representation `checker.lucid` and
  `codegen.lucid` both build from the same AST, shared so the two don't
  duplicate it: `int`, `bool`, `str`, and a named class, plus the
  class/function tables built from a module's declarations. A function
  or `dispatch def` name can have more than one registered signature —
  Lucid's multiple dispatch (see
  [Dispatch](../docs/dispatch.md)) — resolved purely by the *static*
  argument types at a call site, with no runtime dispatch mechanism:
  correct as long as every argument's type is exactly known, which it
  always is in this subset (no subtyping, no unions).
- `checker.lucid` — a static checker over `ast.lucid`'s `Expr`/`Stmt`
  nodes: no execution, just name resolution, call-arity/signature
  resolution, and structural field checking, over the subset
  `codegen.lucid` compiles. Not feature parity with
  `compiler/crates/lucid-checker` (~20,000 lines covering generics,
  traits, inferred variance, exhaustiveness, and far more) — no real
  type inference beyond "is this int/bool/str/a known class", no
  flow-sensitive definite-assignment analysis — but real, useful
  checking: duplicate functions/overloads, undefined names, wrong-arity
  or no-matching-overload calls, unknown classes, wrong field
  count/type, unknown fields, and unsupported types are all rejected
  with a specific message. A binary/unary operator on non-class operands
  has its own required operand types, checked explicitly rather than
  accepted for any two operands of the same non-class kind — arithmetic
  needs two `int` (except `+`, which also accepts two `str`, as
  concatenation), `and`/`or` need two `bool`, `==`/`!=` and ordered
  comparison need two `int` or two `str` (or two `bool`, `==`/`!=`
  only), `not` needs `bool`, unary `-` needs `int` — since
  `codegen.lucid` maps these straight to C's own operators, which
  silently do pointer arithmetic or pointer comparison on two `const
  char *` operands rather than rejecting them (an earlier draft
  accepted any two non-class operands here, which `"a" + "b"` and
  `1 and 2` both slipped through, reaching codegen as real miscompiles
  rather than "unsupported" — `"a" == "a"`/`"a" < "b"`/`"a" + "b"` and
  `len(s)` came later, once codegen actually had a correct C
  translation for each, `strcmp`/`strlen`/`lucid_rt_str_concat`). `for` is
  checked over exactly two iterable
  shapes — a list literal (every element checked, all required to
  agree on one type) and `range(...)` (one or two arguments, always
  `int`) — there's no general `list[T]` typing yet, so anything else is
  rejected as "unsupported iterable expression in this subset"; `for`
  and `while` both accept an `if_broken` clause, checked as an ordinary
  extra block; `print(...)` accepts a class value whose fields are all
  int/bool/str — `codegen.lucid` has a per-class printer for those, see
  below — but rejects one with a class-typed field (no nested printer
  call yet); `freeze(...)` is rejected outright, not emulated (see
  `compile_demo.lucid`'s entry below for why). Every name (a `VarDef`,
  a plain `x = ...` assignment, a `for`-loop target) has exactly one
  type for its whole enclosing function — re-binding a name to a
  *different* type is rejected, and a `VarDef` with both a declared
  type and an initializer must agree — because `codegen.lucid` hoists
  exactly one C declaration per local name to the top of its enclosing
  function; nothing enforced that invariant before, so a function that
  reused a name at two different types would compile to a single
  wrongly-typed declaration. `if`/`elif`/`while` conditions must be
  `bool` (C accepts any scalar there and would silently apply its own
  truthiness instead of rejecting, say, `if 5:`). Every function needs
  an explicit return-type annotation (an unannotated one used to
  default silently to `int`, so a function actually returning `str`
  would compile as if it returned `int`, handing codegen a garbage
  value); every `return` is checked against that type, bare `return` is
  rejected (there's no `none`/void type in this subset for it to mean),
  and a `return` outside any function is rejected too. A function whose
  body can fall off its end without returning is rejected as well — a
  conservative "definitely returns" predicate (an `if` only counts if
  its then-branch, every `elif`, and a *present* `else` all definitely
  return; no `else` at all means falling through is possible), since
  reaching the closing brace of a non-`void` C function without a
  `return` is undefined behavior (C11 6.9.1p12), not just "returns some
  default" — `gcc` without `-Wall` doesn't even warn about it. `break`/
  `continue` outside a loop are rejected too — codegen maps them
  straight to C's own `break`/`continue`, which `gcc` itself does catch
  ("not within loop", loud, not a silent miscompile), but Lucid rejects
  them at check time, and a loop's `if_broken` clause needs its own
  rule here: a `break` inside it refers to whatever loop was already
  enclosing the one that just ran the clause, not the clause's own
  loop (which has already exited by the time `if_broken` runs) — the
  same "outer context" rule `codegen.lucid`'s `broken_flag` threading
  uses for the identical case.
- `checker_demo.lucid` — runs `checker.lucid` against clean programs
  (plain functions; classes with `dispatch def` operator overloading)
  and one program for each kind of error it catches, printing what it
  found.
- `codegen.lucid` — a C code generator for the same subset
  `checker.lucid` accepts: `int`/`bool` are a C `long`, `str` a
  `const char *`, a class a plain (not heap-allocated) C struct passed
  and returned by value; arithmetic/comparison/logical operators map
  straight to their C equivalents (`//`/`%` through two `static` helper
  functions in the generated C's own prelude — Lucid's are *Euclidean*:
  the remainder is always in `[0, |b|)`, unlike both C's truncating and
  Python's floored division, a real, easy-to-miss mismatch this
  codegen's first draft got wrong; `==`/`!=`/ordered comparison on two
  `str` operands go through `strcmp` instead, since two `const char *`
  pointers being `==` in C compares addresses, not contents; `str + str`
  goes through `lucid_rt_str_concat`, another prelude helper
  (`malloc`+`strcpy`+`strcat`, this subset's first heap allocation,
  leaked like everything else here); `len()` of a `str` is `strlen`,
  cast to `long`); a binary operator on two class-typed
  operands, or a call to a name with more than one `dispatch def`,
  resolves to one specific C function chosen by the static argument
  types and name-mangled by them (two `__add__` overloads, on `Vector2D`
  and on `Vector3D`, become `lucid____add____Vector2D__Vector2D` and
  `lucid____add____Vector3D__Vector3D`), always `lucid_`-prefixed — C
  reserves every identifier starting with two underscores, or one
  underscore and an uppercase letter, which Lucid's own dunder
  convention for dispatch operators collides with directly enough that
  the *un*-mangled single-overload case, plain `lucid___add__`, needs
  exactly the same prefix, since C has no overloading of
  its own; every Lucid function is one of these mangled functions,
  including one literally named `main` — C's own `int main(void)` is a
  *separate*, unconditionally synthesized entry point built from the
  module's top-level statements, matching both `lucid run` (which never
  auto-invokes a user-defined `main`, only runs top-level statements —
  the way every real `examples/*.lucid` program is actually written)
  and the reference native backend (`compiler/crates/lucid-codegen`
  always mangles a Lucid `main` too, `lucid_fn_main`, and always
  synthesizes its own C `main` from top-level code). An earlier draft
  special-cased a Lucid function literally named `main` into becoming
  that entry point directly — plausible-looking, and wrong: it
  diverged from what `def main(): ...` followed by top-level code
  actually does in both the interpreter and the reference compiler,
  caught by checking exactly that program compiled versus
  interpreted, not by inspection; `print`
  takes any number of int/bool/str arguments, each formatted by its own
  inferred type — `bool` prints as `true`/`false` (via a C `?:` into
  `%s`), not `1`/`0`, matching the reference interpreter; a class value
  whose fields are all int/bool/str prints as `ClassName({"field":
  value, ...})`, matching lucid-runtime's own default repr, through a
  `static void lucid_print_<ClassName>(<ClassName> value)` emitted
  right after the class's struct, fields sorted by name (matching a
  fix to lucid-runtime's own repr — see below); mixing a class-typed
  `print` argument with ordinary ones switches from one combined
  `printf` call to a C comma expression sequencing each argument's own
  `printf`/`lucid_print_<ClassName>` call, since `print`'s codegen has
  to return one expression (its call site is `codegen_expr`, not a
  statement); a class with a class-typed field can't print yet (no
  nested printer call) and is rejected by `checker.lucid`, honestly,
  rather than silently passed to `printf` as a struct — undefined
  behavior, not just wrong output. `for` over a list literal, or over `range(...)`, both
  compile to a hidden C counter driving the loop, with the visible loop
  target assigned from it at the top of each iteration — never the C
  counter itself, and `range(...)`'s bounds are evaluated once into
  their own C locals before the loop starts, not re-evaluated in the C
  loop condition. Both match Lucid's own per-iteration re-binding
  semantics: a body that reassigns the loop target, or mutates a
  variable `range(...)` read its bound from, must not change how many
  iterations run (an earlier draft used the target itself as the C `for`
  counter, and evaluated the `range(...)` stop bound directly in the C
  condition — a real miscompile on both counts, caught by writing
  exactly those two programs and comparing against the reference
  interpreter before assuming the naive translation was fine). An
  `if_broken` clause on either `for` or `while` compiles to a C `int`
  flag, set right before every `break;` that's actually inside that
  loop (nested loops each get their own flag, or none, matching
  docs/for-and-while.md's "an if_broken clause belongs to the loop
  immediately before it"), checked in an `if` right after the loop
  exits. Lucid has no subprocess/exec builtin, so codegen stops
  at emitting C text — invoking a system C compiler on it is necessarily
  a driver step outside Lucid, the same role
  `compiler/crates/lucid-codegen` itself plays (it also just shells out
  to `gcc`, from Rust rather than from the Lucid it compiles).
- `compile_demo.lucid` — **an actual compiler, written in Lucid,
  compiling real Lucid programs to native executables.**
  `examples/vectors.lucid` — classes, `dispatch def` operator
  overloading, multi-argument mixed-type `print` — was never written for
  this pipeline; lexed, parsed, checked, and compiled to C entirely
  through `lexer.lucid`, `parser.lucid`, `checker.lucid`, and
  `codegen.lucid`. Also compiles eight hand-written programs, each a
  real file under `self-host/compile_demo_programs/` (recursion,
  iteration, `%`/`and`/`or`/comparisons, `//`/`%`'s exact Euclidean
  semantics on all four sign combinations, `bools` — bool literals,
  comparisons, and `and`/`or` results printed as `true`/`false` —
  `loops` — `for` over both a list literal and `range(...)`, a nested
  `for` whose inner loop's `if_broken` clause re-breaks the outer loop,
  a `while` with its own `if_broken`, and the two range-codegen
  miscompile cases described above — `records` — printing a class
  value, alone, mixed with other `print` arguments, and with more than
  one field of mixed str/int/bool type — and `strings` — `==`/`!=`/
  `<`/`<=`/`>`/`>=`, `len()`, and `+` concatenation on `str`, standalone,
  assigned to a variable, and through user functions), plus one Lucid string literal
  that's supposed to fail checking, proving a real error stops codegen
  instead of emitting broken C. `shapes.lucid` from `examples/` isn't
  compiled here (yet) — it needs `freeze()`, rejected outright in this
  subset rather than emulated: lucid-runtime's `freeze` mutates a
  shared `is_frozen` flag on a reference-counted object, and the
  reference native backend tracks the same thing at runtime on a boxed
  value, but this subset's classes are plain by-value C structs with no
  aliasing at all, so neither the interpreter's nor the reference
  compiler's actual `freeze` semantics has anything to attach to here.
  Every valid program's compiled binary is checked the same
  way `vectors`' is: its output must match `lucid run` on that exact
  source file exactly — not a hardcoded expected string — the same
  differential discipline `self_hosting_demo.lucid` already applies to
  the self-hosted lexer and parser (an earlier draft hardcoded each
  hand-written program's expected output instead, the only place in
  this pipeline that wasn't actually differential-tested against the
  reference interpreter). Each valid program's C output is written to
  `self-host/compile_demo_<name>.c` (gitignored — regenerated by running
  the demo); `gcc -std=c11 -o <binary> self-host/compile_demo_<name>.c`
  compiles it, and running the resulting binary produces the exact
  correct output.
  `self_hosted_compile_demo_produces_correct_native_binaries` in
  `compiler/crates/lucid-cli/tests/spec_tests.rs` is that external
  driver and every differential check together, checked in as one test
  — deliberately one, not several, since an earlier split into separate
  tests raced on these same shared output files under `cargo test`'s
  default parallelism. Still a real subset,
  not feature parity with Lucid — `examples/shapes.lucid` needs
  `freeze()`, deliberately unsupported here rather than emulated (see
  above);
  `examples/collections.lucid` needs comprehensions, `list[T]`
  operations beyond a `for` target (`append`, indexing, `len`), dicts,
  and sets; `examples/errors.lucid` needs `?` compiled to some
  checked-error ABI; `examples/hello.lucid` needs string methods and
  comprehensions — none of which
  `checker.lucid`/`codegen.lucid` (or, for some of these, `parser.lucid`)
  support yet. Generated C programs never free anything they allocate —
  there is no garbage collector and no arena; each compiled program runs
  once and exits, so this is a deliberate simplification, the same kind
  as skipping `?`'s checked-error ABI, not an oversight. `str + str`
  (`lucid_rt_str_concat`, in the generated C's own prelude) is this
  subset's first heap allocation, `malloc`+`strcpy`+`strcat`, leaked
  same as everything else — `list[T]` isn't compiled yet, so `append`
  is still the next place this note will need revisiting.

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
- **`if_broken` is a reserved word too**, for the same reason `out` is:
  it's a real Lucid keyword ([For and while
  loops](../docs/for-and-while.md#if_broken-loop-clauses)), so
  `ast.lucid`'s `For`/`While` classes name the field `if_broken_body`,
  not `if_broken`, and `parser.lucid`/`checker.lucid`/`codegen.lucid`
  all use `broken_body` (never `if_broken`) as the local name in a
  `match ... as` binding over it — using the keyword itself as either
  gets the same parse error `out` does. (The field started out named
  `on_exhausted`, copied from a first guess at the semantics before the
  code checked docs/for-and-while.md — `if_broken` runs when the loop
  *did* break, not when it ran to exhaustion, the opposite of Python's
  `for`/`else`; fixed once `for`/`if_broken` codegen actually needed to
  get this right.)
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
- **A real Lucid script has no `main` — its top-level statements just
  run.** `checker.lucid`/`codegen.lucid` first required an explicit
  `def main() -> int:` as the entry point, matching C's own convention —
  fine for hand-written test programs, but every actual
  `examples/*.lucid` program (this repo's own real Lucid source) is
  written the other way, with top-level statements that just execute in
  order, no wrapper function at all. Fixed by having both check and
  compile top-level statements the same way as any function's body (with
  their own fresh module-level scope). A later pass (commit `a97ab9d`,
  "Reject fall-off-end functions; fix a real main() semantics
  divergence") fixed this further: a Lucid function literally named
  `main` was still special-cased into becoming C's own entry point
  directly — `codegen.lucid` now always synthesizes C's `int
  main(void)` from the module's top-level statements alone,
  unconditionally, and a Lucid `main` (if a program happens to declare
  one) is just an ordinary mangled function like any other, never
  auto-invoked, matching both `lucid run` and the reference native
  backend.
- **`parser.lucid` didn't parse `dispatch def` at all.** Needed to parse
  `examples/vectors.lucid` in the first place: `parse_raw_function`
  already threaded an `is_dispatch` flag through to `FunctionDef` (set
  unconditionally to `false`), but `parse_statement` never recognized the
  `DISPATCH` keyword to call it with `true`. A three-line addition
  (`if kind == "DISPATCH": self.advance(); self.expect("DEF")?; ...`) —
  a case of the AST/data plumbing already being ready for a feature the
  grammar-level entry point simply never wired up.
- **A method call inside a large class sometimes reports the wrong
  arity, for reasons not fully root-caused.** `codegen.lucid`'s
  `Codegen` class (about 15 methods) had one method,
  `self.codegen_struct(cls)` (one argument beyond `self`), rejected with
  "method accepts fewer arguments than supplied" — renaming the method
  changed nothing, and trimming its body to a one-line stub didn't
  change the error either, so it wasn't about the name or what the body
  did. `codegen_struct` doesn't actually touch any of `Codegen`'s own
  fields, so moving the identical logic out to a plain module-level
  function (`emit_struct(cls: ClassSig)`, called as `emit_struct(cls)`
  rather than `self.emit_struct(cls)`) made the error disappear — the
  fix this file keeps, and arguably the better design regardless, but
  the underlying checker bug (something about this specific method, in
  this specific class, in this specific position) is still unexplained
  and worth a closer, dedicated investigation later.
- **`print()`ing the same object twice produced two different
  strings.** Found while checking whether a compiled class-value
  printer could even be differential-tested against `lucid run`'s own
  output: `Value::Object`'s fields live in a `HashMap`, whose iteration
  order Rust randomizes per process, so `lucid-runtime`'s own repr
  wasn't reproducible even within a single `cargo test` run, let alone
  across two separate processes (the reference interpreter and a
  compiled binary). A real bug in `lucid-runtime`, not a self-host
  concern as such, but this is exactly what the differential discipline
  this whole pipeline relies on exists to catch — fixed by sorting
  fields by name in `Value`'s `Debug` impl before formatting, and
  `codegen.lucid`'s printer sorts its own field list the same way to
  match.
