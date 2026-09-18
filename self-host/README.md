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

**`self-host/lexer.lucid` and `self-host/parser.lucid` both now compile
through this pipeline**: `checker.lucid` reports zero errors on each,
and `codegen.lucid`'s generated C for each compiles cleanly with `gcc`
— the actual bootstrap target, reached for `lexer.lucid` after closing
the gaps union return types, `match` narrowing, and `?` exposed (see
the `checker.lucid`/`codegen.lucid` entries below, and the
`union_types.lucid` compile-demo program, whose shapes were chosen
specifically to mirror `lexer.lucid`'s own), and for `parser.lucid`
after closing every gap listed in the previous revision of this
section (cross-file imports, class inheritance, union-typed fields, `?`
in three positions, sealed-hierarchy matching, subtype-aware calls,
union return-type subsets) plus five more this branch found by
compiling `parser.lucid` itself all the way through and reading every
resulting `gcc` error, not just the checker's own: `?` hoisted out of a
`Call`/`Construct` argument or a `Binary` operand (see the
`checker.lucid`/`codegen.lucid` entries below), `int(...)`/`float(...)`
conversion builtins and a `FloatType` primitive (`ast.lucid`'s own
`FloatLit`/`ComplexLit` fields, never before supported), a Lucid local
whose name collides with a C reserved word (`default`), a `VarDef`
initialized with a subtype `Construct` into a declared sealed-base
type, and a union type used only as a local variable's own declared
type (neither a return type nor a field). Neither is yet linked and run
end-to-end as a working compiled lexer/parser (that needs a caller — a
compiled `checker.lucid`, or a small hand-written driver — to actually
invoke `tokenize`/`parse` and check the output against the reference
interpreter's own, the same differential discipline every other
compiled program in this pipeline already gets); `self-host/
_probe_selfcompile.lucid` (untracked, a standing local gauge, not
committed) re-runs the `lexer.lucid` half of this check on demand.

**Next target: `self-host/checker.lucid`**, probed the same way.

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
  type inference beyond "is this int/bool/str/a known class/a list of
  one of those", no
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
  checked over three iterable
  shapes — a list literal (every element checked, all required to
  agree on one type), a `list[T]` value (a variable, a field, a call —
  anything whose own type resolves to `list[T]`), and `range(...)` (one
  or two arguments, always `int`) — so anything else is
  rejected as "unsupported iterable expression in this subset"; `for`
  and `while` both accept an `if_broken` clause, checked as an ordinary
  extra block; `print(...)` accepts a class value whose fields are all
  int/bool/str — `codegen.lucid` has a per-class printer for those, see
  below — but rejects one with a class- or list-typed field (no nested
  printer call for either yet) or a list value directly (no printer for
  those at all); `freeze(...)`
  is rejected outright, not emulated (see
  `compile_demo.lucid`'s entry below for why); `s[i]`/`xs[i]` are
  checked for `s: str` or `xs: list[T]` with `i: int`, yielding `str`
  or `T` respectively — anything else is "indexing is only supported on
  str/list[T] in this subset". `list[T]` itself: `T` is int/bool/str/a
  class, including a class defined earlier or later in the same module
  (`list[list[T]]` is rejected outright — `resolve_type_expr` refuses to
  resolve it — a scope decision, not a technical one:
  `mangle_component`/`emit_list_type` are written recursively and would
  emit a `lucid_list_list_int` correctly if asked, but nothing in this
  subset needs nested lists yet); a class-typed
  field can itself be `list[T]` now too, once classes became
  heap-allocated pointers (see `codegen.lucid`'s entry below) rather
  than by-value structs, which is what made a class holding a
  `list[LaterClass]` field a typedef-ordering problem in the first
  place; an empty
  list literal `[]` can only appear where a declared type gives its
  element type away (`xs: list[int] = []`) — anywhere else, "cannot
  infer the element type of an empty list literal in this subset";
  `len(...)` accepts a `str` or a `list[T]` (not just `str`, as
  before); `xs.append(v)` requires `v`'s type to exactly match `xs`'s
  declared element type; `xs.pop()` (no arguments) removes and returns
  the last element, typed `T`, matching `list.pop()`'s reference
  semantics (verified first: `[1, 2, 3].pop()` returns `3`, leaving
  `[1, 2]`) — `append`/`pop` are the only `list[T]` methods. `dict[str,
  V]` for `V` in int/bool/str — always a `str` key, never a type
  parameter of its own, matching `lucid-runtime`'s own `Value::Dict`
  (a `HashMap<String, _>` regardless of the declared key type); a dict
  literal's keys must all be `str`, its values all the same type,
  matching the same rule list literals already have; an empty `{:}`
  has the same declared-type-only rule as an empty `[]`. `d.get(key,
  default)` is the only `dict[str, V]` method, checked for `(str, V)`
  arguments and returning `V` — the actual feature
  `self-host/lexer.lucid` needs (its own `keywords`/`single` tables).
  `append`/`pop`/`get`, plus class methods (see below), are the only
  method calls this subset supports. A module-level variable (`x: int
  = 5` at the top of a file, or `keywords: dict[str, str] = {...}`) is
  readable inside every function and method body — verified directly
  first (`x: int = 5` then `def f() -> int: return x + 1` prints `6`
  under `lucid run`) — but only if the module-level declaration comes
  *before* the function/method in the source: a function reading a
  module-level name declared *later* in the file is rejected by the
  reference checker (`undefined variable`), unlike a function/class,
  which can freely forward-reference another one declared later. This
  subset doesn't enforce that ordering — every module-level
  declaration is visible to every function/method regardless of
  source order, a real, deliberate over-permissiveness relative to the
  reference checker, not attempted here (this subset's `check_module`
  already checks every function/class in one forward-reference-
  tolerant pass; enforcing declaration order for module-level
  *variables* specifically would mean checking them interleaved with
  the functions that read them, undoing that simplification for a case
  `self-host/lexer.lucid` itself never needs, since its own
  `keywords`/`single` tables are always declared before anything that
  reads them). A name that's *read* inside a function/method, never
  assigned there, sees the module-level value; a name that's
  *assigned* there gets its own local, shadowing the module-level one
  for the rest of that call, matching docs/scope.md's "assignment is
  always local" rule exactly (verified: `count: int = 0` then `def
  bump() -> int: count = 5; return count` returns `5` without changing
  the module-level `count`) — except for the one case this subset
  doesn't replicate either: the reference checker treats a name
  assigned *anywhere* in a function as local for the *whole* function,
  so reading it before that assignment is itself an error (`count =
  count + 1` is rejected as "undefined variable 'count'", the same
  shape as Python's own `UnboundLocalError`) — this subset's simpler
  copy-the-module-scope-in seeding doesn't catch that case (it would
  silently read the *module-level* value instead of erroring), a real,
  narrower gap than the ordering one above, also not needed by
  anything this subset actually compiles.
  A function or `dispatch def` named
  `print`/`range`/`len`/`freeze` is rejected outright, since
  `check_call`/`codegen_expr` recognize those names ahead of consulting
  the registered function table — a user redefinition would register a
  real signature but still compile as the builtin, silently diverging
  from the interpreter (which does let a later definition shadow one;
  also verified a `dispatch def __add__(a: str, b: str)` overload is
  simply never consulted for `"x" + "y"` there either — builtin-typed
  operators aren't dispatchable in the interpreter, so this subset's
  codegen ignoring any such registered overload for `str`/`int`/`bool`
  operands already matches, nothing to fix).
  Every name (a `VarDef`,
  a plain `x = ...` assignment, a `for`-loop target) has exactly one
  type for its whole enclosing function — re-binding a name to a
  *different* type is rejected, and a `VarDef` with both a declared
  type and an initializer must agree — because `codegen.lucid` hoists
  exactly one C declaration per local name to the top of its enclosing
  function; nothing enforced that invariant before, so a function that
  reused a name at two different types would compile to a single
  wrongly-typed declaration. `if`/`elif`/`while` conditions must be
  `bool` (C accepts any scalar there and would silently apply its own
  truthiness instead of rejecting, say, `if 5:`). A function's return
  type is either an explicit annotation or, if omitted, `-> none`
  (verified directly against the reference interpreter: `def f():
  print(1)` and `def f() -> none: print(1)` behave identically there —
  this subset used to reject an omitted annotation outright, back when
  there was no way to represent `none` correctly and silently
  defaulting to `int` was the alternative, a real miscompile, fixed
  before this); every `return` is checked against that type — bare
  `return` is allowed only when the function's return type is `none`,
  rejected otherwise — and a `return` outside any function is rejected
  too. `none` itself is only ever a return type here, never a value: a
  call to a `-> none` function can only appear as its own statement,
  never assigned to a variable or passed to `print(...)`. A function whose
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
  uses for the identical case. `p.field = value` (attribute
  assignment, not just attribute *read*) is checked against the
  field's declared type the same way a plain field access already was
  — required for a class method to mutate its own state, since a class
  became a heap-allocated pointer (see `codegen.lucid`'s entry below)
  specifically so a mutation through one alias is visible through
  every other alias of the same object, matching `lucid-runtime`'s own
  `Value::Object` semantics (verified directly: a function taking a
  class argument and assigning to one of its fields changes the
  *caller's* object, not a copy). A class body can now have methods
  (`def method(self, ...): ...`), not only fields — `self` must be
  bare, never `self: ~Self`/`self: !Self` (matching every method in
  `self-host/lexer.lucid` itself, the actual target this subset needs
  to keep growing toward), and its type is always implicit, the
  enclosing class's own `ClassType`, never stored in the method's own
  `FuncSig.param_types` (which holds only the parameters *after*
  `self`) — the receiver at each call site supplies it instead, the
  same way `codegen.lucid` builds a method's C parameter list
  (`ClassName *self` first, from the class's own name directly, then
  the rest). No overloading for methods in this subset (one method per
  name per class, `dispatch def` is a *function*-level, not a
  *method*-level, feature here); a call `obj.method(args)` on a
  class-typed `obj` resolves `method` against that class's own method
  table and checks `args` against its parameter types, the same shape
  `list[T].append` already used for its one supported method.
  A function or method's return type can now be a union
  (`Token | none`, `Token | LexError`) — `types.lucid`'s new
  `UnionLType`, produced only from a `-> A | B` annotation
  (`resolve_type_expr`'s new `UnionType` case, never from a field,
  parameter, list element, or dict value position, which all explicitly
  reject it), the actual gap that blocked `self-host/lexer.lucid` itself
  from compiling at all before this (`handle_indentation`, `lex_string`,
  `next_token`, and `tokenize` all return a union). A bare `x =
  some_union_returning_call()` binds `x` directly to the whole union (a
  real, representable value here, unlike `none` — see UnionLType's own
  header comment in `types.lucid`); `self-host/lexer.lucid`'s own
  `layout = self.handle_indentation()`, narrowed only by the `match`
  that reads it next, is exactly this shape. Two ways to narrow one: a
  `match` statement, and `?`.
  **`match`** requires a union-typed subject (`check_match` rejects any
  other type outright — codegen has no tag to switch on otherwise); each
  arm's pattern is either a `case ClassName:` (a `TypePattern`, checked
  via `assignable_to` against the subject's own members) or the literal
  `case none:` (a `LiteralPattern(NoneLit)`) — the two shapes
  `self-host/lexer.lucid`'s own `match layout as result: case Token: ...
  case none: ...` uses — or a wildcard `case _:`; no other literal
  pattern, and no arm guard (`case X if cond:`), is supported. The
  subject's own `as` alias, if present, is bound to the narrowed type
  *inside that arm's own body only* — never merged back into the
  enclosing scope — since it has a *different* type in each arm, which
  would otherwise violate this subset's "one type per name per function"
  invariant (see `bind()`'s own comment); nothing in
  `self-host/lexer.lucid` ever reads the alias again after its own
  `match` anyway. Exhaustiveness is checked: every member of the
  subject's union needs its own arm (by name or by wildcard), matching
  the reference checker's own requirement and giving codegen's
  tag-check chain nowhere to silently fall through.
  **`?`** (`check_propagate`) requires its operand's type to be a union
  with *exactly* one "error" member (a class whose name ends in
  `"Error"` — the same heuristic `self-host/interpreter.lucid` and
  `compiler/crates/lucid-checker`'s own `Propagate` handling use) and
  exactly one non-error member — narrower than the reference checker,
  which allows more than one error member; see this method's own
  comment and the Design notes below for why. It requires the enclosing
  function's own return type to already include the propagated error
  type (`assignable_to`, the same check a plain `return` uses), and
  evaluates to the single non-error member's type. `check_rhs_expr`
  restricts `?` to appearing only as the direct right-hand side of a
  plain assignment (`tok = lexer.next_token()?`, `tokenize`'s own
  shape) — not nested inside a larger expression, and not as a
  `return` statement's own value either (`return parse(text)?`,
  `docs/question-mark-operator.md`'s own example, which checks out
  under the full reference checker but isn't one of the two statement
  shapes `codegen.lucid`'s own desugaring implements — see its entry
  below); every other appearance of `?` reports a specific error rather
  than falling through to "unsupported expression kind".
  `check_construct` also gained the same bidirectional handling VarDef's
  empty list/dict literal already had, for a constructor argument
  specifically: `Stack([], 0)`'s empty `[]` argument has nothing of its
  own for `check_expr` to infer an element type from, so
  `check_construct` now infers it from the *target field's* own
  declared type (`items: list[Item]`) instead, the same way a VarDef's
  declared type already could — a real gap only surfaced by
  `union_types.lucid`'s own `Stack` class, never hit by any earlier
  `compile_demo_programs/*.lucid` file.
  `bind()` also now accepts *narrowing* a name whose established type
  (from its own `VarDef`'s declared type, or an earlier assignment) is a
  union, down to one of its members — `x: Item | none = none` then
  later `x = Item(1)`, or back to `x = none` — instead of rejecting it
  as a type change; the established union stays the name's own type in
  `known` either way (a later read, or a later reassignment to a
  *different* member, still needs to see the full union), and
  `codegen.lucid`'s VarDef/Assignment codegen wraps the narrower value
  into that union at the assignment site, the same way a `return`
  already does. The reverse direction — a name already established as a
  plain, non-union type later assigned a union-typed value — is still
  rejected: `assignable_to` is asymmetric on purpose (a union can
  satisfy a narrower expectation only by first being narrowed, never the
  other way). This is `self-host/parser.lucid`'s own dominant pattern
  (`value: Expr | none = none` then `value = self.parse_expr()` deeper
  in the same function) and was the next thing probing it surfaced after
  `self-host/lexer.lucid` itself started compiling (see Status above).
  `check_module` now resolves `from .x import ...` too, the other thing
  probing `self-host/parser.lucid` surfaced (it imports nearly every
  class in `self-host/ast.lucid` and `tokenize` from `self-host/
  lexer.lucid` directly) — every "unknown class" error checking it used
  to produce is gone as of this. `types.lucid`'s new
  `resolve_all_declarations` (shared with `codegen.lucid`, see its own
  entry below) reads and parses every transitively-imported file
  (`read_file`/`parse`, the same builtins `compile_demo.lucid` itself
  already uses to drive this whole pipeline — deliberately not a real
  module system, just "read the file, parse it, recurse", resolved
  directly against `self-host/` since every self-hosted file only ever
  imports a sibling in this same directory) and returns every one of
  its top-level statements (a class/function declaration, registered
  the same way one declared directly in this module already is; a
  module-level `VarDef`/`Assignment` — `self-host/lexer.lucid`'s own
  `keywords`, which several of its methods read — checked exactly like
  this module's own top-level statements, since that's the only place
  its type ends up in `module_scope` at all); the imported declarations
  themselves aren't separately re-checked here (their own file is
  checked on its own, when it's compiled directly). `collect_class`
  also gained a `pass`-bodied class case (`class Skip(Expr): pass`, a
  zero-field, zero-method marker type — `self-host/ast.lucid` alone has
  over a dozen of these), a real gap surfaced the moment any import from
  it pulled one in; previously rejected as "only plain fields and
  methods ... are supported in a class body", now treated as a class
  with an empty `fields`/`methods`, matching how `Construct` for one
  already takes zero arguments. This resolution is deliberately
  name-blind — every declaration in an imported file is registered,
  not just the ones a `from .x import a, b` statement actually names —
  simpler than tracking which imported names a program is allowed to
  use, and codegen needs the *whole* transitively-imported file inlined
  regardless (see below), so checker.lucid doing the same keeps the two
  in step; the cost is a real but narrow honesty gap (a program that
  constructs a class it never imported, but that its actual import
  *did* transitively pull in, wouldn't be caught) — not a soundness
  problem for what codegen can compile, just a missed diagnostic.
  A class can now extend one other class (`class Binary(Expr):`) —
  `self-host/ast.lucid`'s own node classes all extend a `sealed class
  Expr:`/`sealed class Stmt:`/etc. with its own field (`span: Span`),
  the shape cross-file imports started actually registering. `ClassSig`
  gained a `parent: str | none` field (`resolve_parent`, resolving
  `cd.bases[0]` the same way any other type annotation is, and
  rejecting more than one base outright — no traits in this subset) and
  `fields` now includes the parent's own fields first, via `types.lucid`'s
  new `inherited_fields` — `Binary(span, left, op, right)`, not
  `Binary(left, op, right)`, the same construction order `Construct`
  checking already validates positionally. A new `class_assignable_to`
  (also `types.lucid`, alongside a plain `is_subclass_of` walking
  `ClassSig.parent` up the chain) extends `assignable_to` with subtype
  awareness, used everywhere a concrete class value meets an expected
  type: `check_construct` (a `LiteralExpr` argument satisfies a field
  declared `Expr`), `check_stmt`'s Return arm and Attribute-assignment
  arm, and `bind()`'s own union-narrowing check, one level narrower now
  too (a name established as a plain `Expr` can be reassigned a
  `LiteralExpr` later, keeping `Expr`, the same "keep the wider type"
  rule the union case already had). `assignable_to` itself stays
  ignorant of classes on purpose — almost none of its own many call
  sites ever compare two class types, so only the handful that do call
  `class_assignable_to` instead.
  Matching one back *down* to its own concrete subtype (`match
  node_value as n: case NumberNode: ...` where `node_value`'s own
  static type is a `sealed class Node:`, not a union) is a real, second
  runtime tag now too — `ClassSig` gained `sealed_root: str | none`
  (itself, for a `sealed class`; its parent's own `sealed_root`,
  inherited down the chain, for an ordinary subclass; `none` outside any
  sealed hierarchy) and a new `class_tag` (`types.lucid`, purely
  derived: every class sharing a `sealed_root`, sorted by name, gives
  each its own index). `check_match` gained a whole second case
  alongside its existing union one — `check_sealed_match` narrows each
  arm to a *concrete* class in the same hierarchy via a plain
  class-name pattern, exactly the shape every one of
  `self-host/parser.lucid`'s own three sealed-hierarchy `match`
  statements already takes (`case Ident: ... case _: ...`) — but
  doesn't attempt real exhaustiveness the way the union case does
  (`self-host/ast.lucid`'s own hierarchies run to dozens of concrete
  subclasses); a trailing `case _:` is required instead. `codegen.lucid`
  gained the matching `codegen_sealed_match` (its own entry below).
  A class field can be a union type now too (`alias: str | none`,
  `self-host/ast.lucid`'s own dominant field shape — `collect_class` no
  longer rejects it) — by-value, the same tagged struct a union return
  type already gets. `?` is now supported in two more positions: a
  `return` statement's own value (`return self.parse_expr()?`,
  `docs/question-mark-operator.md`'s own example shape) and a bare
  expression statement on its own (`self.consume_stmt_end()?`,
  `self-host/parser.lucid`'s single most common `?` shape by a wide
  margin — validating something and discarding the ok value, typically
  `none`, either way) — still never nested in a larger expression, for
  the same "no GNU statement expressions" reason as before (see
  `codegen.lucid`'s own entry below). A real, previously-undiscovered
  bug in `compiler/crates/lucid-runtime` surfaced verifying the
  return-statement shape: `return expr?` breaks the *reference
  interpreter itself* whenever `?` actually propagates (not when it
  unwraps successfully) — documented at length in the Design notes
  below, since it meant that one shape's own failure path couldn't be
  differentially verified against `lucid run` the usual way; the bare
  expression statement shape doesn't share the bug (verified directly,
  both paths) and is differentially tested normally.
  A subtype argument now satisfies a supertype-typed parameter almost
  everywhere a call happens, not just at a `Construct`/`Return`/
  Attribute-assignment site — a real gap the very first sealed-hierarchy
  test surfaced (`describe(some_number_node)` against `def
  describe(n: Node) -> int:` failed with "no overload... matches the
  given argument types", since `find_signature`'s own overload
  resolution only ever checked for an *exact* parameter-type match, the
  right rule for picking among `dispatch def` overloads but too strict
  for an ordinary call). A new `find_signature_assignable` (`types.lucid`)
  tries an exact match first (so overload resolution keeps picking the
  single most specific signature when the argument types line up
  exactly) and only falls back to a subtype-compatible match, in
  registration order, when nothing matches exactly — used in place of
  `find_signature` for a plain function call (`check_call`'s `Ident`
  case). `type_list_assignable` (also `types.lucid`) is the same idea
  for a method call, where there's no overload set to disambiguate at
  all (one method per name per class); `list[T].append(...)`'s own
  element-type check switched from `types_equal` to `class_assignable_to`
  directly for the same reason.
  `class_assignable_to` also grew a case for `value_type` itself being a
  union — satisfied when *every* one of its own members satisfies
  `expected` (recursing into itself per member, so a narrower member
  that's a genuine subtype still works too): `self-host/parser.lucid`'s
  own `def parse(...) -> ParsedModule | ParserError | LexError: ...
  return parser.parse_module()` needs this, since `parse_module`'s own
  declared return type is the strictly narrower `ParsedModule |
  ParserError` (no `LexError` on that path). `codegen.lucid` needed real
  work for this one, not just a type-check relaxation — see its own
  entry below.
  `?` nested one level inside a `Call`/`Construct` argument or a
  non-short-circuit `Binary` operand is now supported too (`self-host/
  parser.lucid`'s own dominant remaining `?` shape, eleven of eighteen
  occurrences on the last probe before this branch: `body.append(self.
  parse_class_member()?)`, `module_name = module_name + self.
  parse_dotted_name()?`) — by hoisting, not by teaching `check_expr`
  itself about nested `?`. A new `hoist_propagate` (`ast.lucid`, shared
  with `codegen.lucid`) looks one level into an expression's own direct
  children for a single `Propagate` node and, if it finds one, returns
  an equivalent tree with that node replaced by a fresh `Ident`
  referencing a temp that already holds its unwrapped ok value —
  `check_rhs_expr` then type-checks the *substituted* tree exactly like
  any other expression (an `Ident` with a known type needs no special
  case anywhere), after checking the extracted `Propagate` itself via
  `check_propagate` and registering the temp's ok type. Deliberately
  narrow: only a `Call`/`Construct` argument or a `Binary` operand, one
  level deep, and `and`/`or` are skipped on purpose (their right operand
  isn't always evaluated, so hoisting it out ahead of the operator would
  change what actually runs) — nested any other way (an `if`-expression
  branch, a comprehension, two levels deep) still isn't supported, and
  still reports the ordinary "'?' is only supported as..." error.
  `int(...)`/`float(...)` are now recognized as two narrow builtin
  conversions (`check_call`, a single `str` argument each, returning
  `int`/the new `FloatType`) — `self-host/parser.lucid`'s own
  `IntLit(int(tok.text))`/`FloatLit(float(tok.text))`. `FloatType` itself
  (`types.lucid`) exists purely so `ast.lucid`'s `FloatLit.value`/
  `ComplexLit.value: float` fields can be registered at all (`collect_
  class` previously rejected them outright, which — since `check_
  construct` only iterates as many fields as `cls.fields` actually has —
  silently dropped `FloatLit`'s field list to zero and never even
  reached checking `float(tok.text)` as a Construct argument); there's no
  float arithmetic, comparison, or dispatch overload anywhere in this
  subset, since nothing self-hosted needs one yet.
- `checker_demo.lucid` — runs `checker.lucid` against clean programs
  (plain functions; classes with `dispatch def` operator overloading)
  and one program for each kind of error it catches, printing what it
  found.
- `codegen.lucid` — a C code generator for the same subset
  `checker.lucid` accepts: `int`/`bool` are a C `long`, `str` a
  `const char *`, a class a heap-allocated, *pointer*-typed C struct
  (`ClassName *`), matching `lucid-runtime`'s own `Value::Object`
  (`Rc<RefCell<HashMap>>` — reference semantics, an alias mutates every
  alias's fields) — every class was a plain *by-value* struct through
  the first several commits of this pipeline, safe only because
  nothing could mutate a field after construction; `list[T]` needed
  the same representation for the same reason (see below), and once
  method bodies needed to mutate `self.field` in place (a later
  commit), by-value classes stopped being representable at all, so
  every class switched too. A class emits a `typedef struct Name Name;`
  forward declaration before its full `struct Name { ... };` body — all
  of a module's forward declarations come first, in one pass, so one
  class's field can hold a pointer to another class declared earlier
  *or later* in the same module, a typedef-ordering question a named
  struct tag with a pointer field sidesteps entirely (an anonymous
  by-value struct couldn't). `Construct` (`ClassName(args)`) compiles
  to `lucid_new_ClassName(args)`, a generated constructor doing exactly
  one `malloc` and one field assignment per argument — leaked, like
  every other allocation in this subset. Attribute access is `->`, not
  `.`. `list[T]` element types and class fields can both be another
  class now (`Circle *` fits in a `lucid_list_Circle`'s `items` array,
  or another struct's field, the same way any other pointer does) —
  though `is_printable_class` still refuses to print one (a class- or
  list-typed field needs its own nested printer call, not supported
  yet); arithmetic/comparison/logical operators map
  straight to their C equivalents. `//`/`%` go through two `static` helper
  functions in the generated C's own prelude — Lucid's are *Euclidean*:
  the remainder is always in `[0, |b|)`, unlike both C's truncating and
  Python's floored division, a real, easy-to-miss mismatch this
  codegen's first draft got wrong; `==`/`!=`/ordered comparison on two
  `str` operands go through `strcmp` instead, since two `const char *`
  pointers being `==` in C compares addresses, not contents; `str + str`
  goes through `lucid_rt_str_concat`, another prelude helper
  (`malloc`+`strcpy`+`strcat`, this subset's first heap allocation,
  leaked like everything else here); `len()` of a `str` is `strlen`,
  cast to `long`; `s[i]` goes through `lucid_rt_str_index`, a third
  prelude helper — a negative index counts from the end (matching the
  reference interpreter), and unlike `+`/`len()` it doesn't allocate: a
  static `char[256][2]` table of every possible one-character string,
  each already NUL-terminated by C's own zero-initialization of static
  storage, returns a stable pointer per byte value instead. `list[T]`
  is a monomorphized, heap-allocated, *pointer*-typed C struct per
  element type — `lucid_list_int`, `lucid_list_str`,
  `lucid_list_Circle` (named by `mangle_component`, `types.lucid`'s
  `type_name` sibling for building a valid C identifier —
  `type_name(list[int])` is `"list[int]"` for diagnostics, brackets and
  all, which isn't a legal C identifier fragment), each with its own
  `_new`/`_append` (realloc-doubling, leaked)/`_get` (bounds-checked
  exactly like `lucid_rt_str_index`, including negative indices)/`_of`
  (a non-empty list literal's elements, via a C99 compound-literal
  array)/`_pop` (decrements `len` and returns the element that's now
  past it — no `realloc` to shrink, so a popped slot's memory stays
  allocated, consistent with this subset never freeing anything).
  *Pointer*, not by-value, unlike every other type this codegen
  emits: `Value::List` in lucid-runtime is `Rc<RefCell<Vec<Value>>>`,
  reference semantics — verified directly (`b = a; b.append(3)` changes
  `len(a)` too) before choosing this representation, since every
  class so far had been a plain value type specifically because its
  fields can't be mutated after construction, and a list is the first
  type in this subset where that assumption doesn't hold.
  `lucid_list_int`/`_bool`/`_str` are emitted unconditionally, in the
  prelude, regardless of whether a compiled program actually uses
  `list[int]` — cheap, and skips walking a whole program collecting
  which element types are actually instantiated (which could miss one);
  `lucid_list_<Class>` is emitted right after `Class`'s own struct.
  `dict[str, V]` gets the same by-value-list-of-considerations pointer
  representation and unconditional `int`/`bool`/`str`-value emission
  (`lucid_dict_str_int`/`_bool`/`_str`, no `_<Class>` version — this
  subset's `dict[str, V]` doesn't support a class value type at all,
  narrower than `list[T]`'s own element-type scope, since nothing needs
  one yet): a struct holding parallel `keys`/`values` C arrays and a
  `len`, with `_of` (built the same way as a list literal, from two
  C99 compound-literal arrays) and `_get(dict, key, default)` — a
  *linear* `strcmp` scan, not a hash table, since this subset only ever
  compiles small, mostly compile-time-constant tables
  (`self-host/lexer.lucid`'s own `keywords`/`single`), not a
  general-purpose dict.
  A binary operator on two class-typed
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
  `static void lucid_print_<ClassName>(<ClassName> *value)` emitted
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
  exits. `p.field = value` compiles to `p->field = value;` — a plain C
  field write through the pointer, not a local-variable declaration, so
  it's the one `Assignment` shape `collect_local_names`/
  `infer_local_types` deliberately skip (there's no local name here to
  hoist a declaration for). A function whose return type is `none`
  compiles to a `void` C function; a bare `return;` inside one compiles
  to a bare `return;` in C, same as it always could have, now that
  `none` is checked as a real return type instead of being rejected
  outright. A method (`def method(self, ...): ...`) compiles to an
  ordinary C function taking `ClassName *self` as an explicit first
  parameter, built directly from the class's own name rather than
  stored in the method's own parameter list (see `checker.lucid`'s
  entry above) — named by a small `method_c_name(class_name,
  method_name)` free function (`"lucid_method_" + class_name + "_" +
  method_name`), deliberately not routed through `mangled_name`/the
  function table a free function's own overloads go through, since a
  method can't be overloaded here (one method per name per class) and
  this naming can never collide with a mangled free function or
  another class's same-named method; `obj.method(args)` compiles to a
  plain C call with the receiver prepended to the argument list. A
  module-level variable (`x: int = 5` at the top of a file) compiles
  to `static <ctype> x;`, an ordinary C file-scope global, emitted
  right after every struct/list/dict type it could reference but
  before any function/method forward declaration; C forbids a
  non-constant initializer on a `static` global (a
  `lucid_dict_str_str_of(...)` call isn't one), so each global's
  actual initialization happens as an ordinary statement inside the
  synthesized C `main()` instead, in the same place top-level code
  already runs. `codegen_body` gained an `exclude: set[str]`
  parameter so `main()`'s own hoisted-locals pass doesn't redeclare a
  name that's already a real C global — only `codegen_entry_point`
  passes a non-empty one, built from `self.module_scope`'s own keys.
  Every function's and method's own local type-scope is seeded with a
  copy of `self.module_scope` (via a small `copy_type_scope` free
  function, duplicated identically in `checker.lucid` and
  `codegen.lucid`, matching this pipeline's no-shared-helpers-between-
  files pattern) so a function's own local reassignment of a
  same-named variable never leaks back into the shared module scope.
  Module-level names are iterated in sorted order everywhere codegen
  depends on that order (both the `static` declarations and the
  init statements in `main`), since a Lucid `dict`'s own key order
  isn't guaranteed stable, matching an earlier determinism fix to
  lucid-runtime's own object repr.
  A union return type (`Token | LexError`) compiles to a small, *by-value*
  (not heap-allocated) tagged C struct — unlike every reference-semantics
  type this codegen otherwise emits (a class, `list[T]`, `dict[str, V]`),
  a union value only ever exists transiently, between the call that
  produces it and the `match`/`?` that narrows it immediately, so nothing
  in this subset ever aliases one (`emit_union_type`). Its members are
  canonicalized into a fixed order first — sorted by
  `mangle_component`, via `types.lucid`'s own `sorted_union_members`, the
  same reason module-level names are sorted before emission — so `A | B`
  and `B | A` (if either spelling appeared) name the same struct
  (`union_c_name`); the struct is `{ long tag; <ctype> v<i>; ... }`, one
  field per *non-`none`* member at its own canonical index (`none`
  itself carries no data, so it gets a tag value but no field, matching
  `NoneType` having no C representation as a value anywhere else in this
  codegen either). `wrap_union_value`/`union_member_index` build a
  compound literal for a concrete member value (`return LexError(...)`
  from a `-> Token | LexError` method becomes `(lucid_union_LexError_
  Token){.tag = 0, .v0 = lucid_new_LexError(...)}`) — codegen_stmt's
  Return arm calls this whenever the returned expression's own inferred
  type is a *member* of the declared union return type, but skips it
  when the expression's type already *is* that exact union (`return
  self.next_token()` from inside `next_token` itself, a real shape in
  `self-host/lexer.lucid`: a method returning its own recursive call) —
  wrapping an already-wrapped union a second time doesn't type-check in
  C at all, a real bug caught by compiling `self-host/lexer.lucid`
  itself, not by any hand-written `compile_demo_programs/*.lucid` file
  (see the Design notes entry below). Every distinct union return type
  any registered function/method signature actually uses gets its own
  struct, emitted once each (`Codegen.collect_union_types`, deduplicated
  and sorted the same way the module-scope globals are) after every
  class/`list[T]`/`dict[str, V]` type a member could reference but before
  any function/method forward declaration.
  `match` on a union subject (`codegen_match`) evaluates the subject
  once into its own temp, then compiles to an if/else-if chain on that
  temp's `.tag` field — deliberately *not* a C `switch`: this subset's
  `break`/`continue` map straight to C's own, and a `break` inside a
  match arm that's itself inside an enclosing `while`/`for` has to mean
  that loop, never the match — a `switch` would silently steal it
  instead. Each arm's own narrowed alias (`case Token: ... uses result
  ...`) is declared as a plain C local *inside that arm's own `{ }`
  block* (`codegen_match_arm`), never hoisted to the function's shared
  locals the way an ordinary variable is: it has a different C type in
  each arm, which would violate this subset's "one C declaration per
  local name" invariant if hoisted the normal way — `collect_local_names`/
  `infer_local_types` both gained a `MatchStmt` case that recurses into
  each arm's own body (so an *ordinary* local a match arm happens to
  declare still gets hoisted) while leaving the alias itself alone.
  `?` (`codegen_propagate_assignment`) only compiles as the direct
  right-hand side of a plain assignment (matching checker.lucid's own
  restriction) into three C statements: a temp holding the propagated
  call's own union value, an `if` on the temp's error tag that returns
  early (wrapping the error into the *enclosing* function's own union
  return type, which can differ from the propagated call's union — this
  is exactly `tokenize`'s own shape: `Token | LexError` propagated
  inside a function returning `list[Token] | LexError`), and a plain
  assignment unwrapping the temp's ok-tagged field into the assignment's
  target. No GNU statement-expression anywhere — this subset holds to
  plain C11, so `?` never appears where only a single C expression is
  legal (nested in a larger expression, or a `return`'s own value); see
  checker.lucid's `check_rhs_expr`, which enforces the same restriction.
  `codegen_module` now resolves `from .x import ...` too, via the same
  `types.lucid` `resolve_all_declarations` checker.lucid's own
  `check_module` uses (see its entry above) — but unlike
  checker.lucid, codegen actually has to *emit* C for an imported
  declaration, not just register its signature: `self-host/parser.lucid`
  calls `tokenize`, imported from `self-host/lexer.lucid`, so the
  generated C for `parser.lucid` needs `tokenize`'s own C function (and
  everything it calls — `Lexer`'s methods, `Token`/`LexError`'s
  structs, `keywords`, ...) inlined directly into the same file, the
  same way it would need to be if there were no separate
  compilation/linking step at all (there isn't one in this subset).
  Every pass that already walks the module's own class/function
  declarations to emit a struct, forward declaration, or body now walks
  the imported declarations first, then this module's own, instead
  (`all_decls`); a module-level `VarDef`/`Assignment` among the
  imported statements is different — a real C global that needs
  declaring *and* initializing, not a declaration these passes know how
  to skip — so it's added to `top_level` instead, ahead of this
  module's own (an imported global has to exist before anything in the
  importing module's own top-level code could read it). Verified with
  `self-host/imports_demo.lucid` — `from .lexer import is_digit,
  is_alpha`, calling both — compiled, `gcc`-compiled, and diffed against
  the reference interpreter: the generated C (about 40KB) inlines all of
  `self-host/lexer.lucid`'s own classes, methods, and free functions,
  including `keywords`, even though `imports_demo.lucid` itself only
  ever calls two of `lexer.lucid`'s dozen-plus functions — the
  name-blind resolution checker.lucid's own entry above explains, here
  for the same reason (codegen needs the whole transitively-imported
  file regardless of which names were actually imported, since anything
  imported might call anything else in its own file).
  A subclass's own struct now includes its parent's fields first
  (`ClassSig.fields`, via `types.lucid`'s shared `inherited_fields` —
  see checker.lucid's own entry above), so `emit_struct`/
  `emit_constructor` need no changes of their own to lay one out or
  build its constructor correctly — they already just iterate
  `cls.fields` in order. What *does* need a change: a field declared as
  a sealed base (`left: Expr`) constructed, returned, matched, or
  field-assigned from a concrete subtype value (`LiteralExpr`) needs an
  explicit C cast, since `LiteralExpr *` and `Expr *` are two separate,
  unrelated struct types here (no C-level inheritance at all) — safe
  under C11 6.5.2.3p6's "common initial sequence" rule for any access
  to a field the *base* class itself declares (`Expr`'s own fields are
  always the first fields of every subclass's own struct too, via
  `inherited_fields`), but never safe to cast back down to a
  *different* concrete subtype without a real runtime tag, which this
  subset doesn't have. `codegen_expr`'s Construct case, `codegen_stmt`'s
  Return and Assignment (Attribute-target) arms, and a new
  `find_union_member` helper (used wherever a value gets wrapped into a
  union, so a `LiteralExpr` satisfies a `Token | none`-shaped union
  whose actual member is `Expr`, not `LiteralExpr` itself) all emit this
  cast where needed. A union-typed field gets the exact same treatment
  as a union-typed local or return value everywhere it's written
  (`emit_struct` already handles the *type* for free — `c_type_of`
  already returns the tagged struct's own name for a `UnionLType`
  field, same as any other type) — but `collect_union_types` had to
  grow a field-scanning pass too, not just its original scan of
  function/method return types, and `codegen_module`'s own emission
  order had to change: a union struct now has to exist *before* any
  class's own full struct body (a union-typed field needs it declared
  already), not just before any function/method forward declaration the
  way a union return type alone required — so `lucid_list_<ClassName>`
  for every class (itself only needing a forward declaration, not a
  full struct, the same reasoning list types have always used) moved
  ahead of union emission too, and full class struct bodies now come
  last of the three. `?` as a `return` statement's own value
  (`codegen_propagate_return`) mirrors `codegen_propagate_assignment`
  exactly for the temp/tag-check/early-return shape, but the final "ok"
  branch returns the unwrapped value directly instead of assigning it,
  wrapped into the enclosing function's own return type the same way an
  ordinary `return` expression already is. `?` as a bare expression
  statement (`codegen_propagate_exprstmt`) is the same shape again, minus
  even that final return — the ok value simply isn't read out of the
  temp anywhere, since there's nowhere for it to go (an ordinary
  expression statement already discards its own value the same way).
  Matching a sealed class hierarchy by its own concrete subclasses
  (`codegen_sealed_match`, alongside the existing union-dispatch
  `codegen_match`) needs a real runtime tag, unlike everything else this
  codegen has built so far (a union's own tagged struct is a compiler
  invention with no equivalent in the source language; a sealed
  hierarchy's concrete identity is real). Every class whose own
  `ClassSig.sealed_root` is set gets a hidden `long __lucid_tag;` field,
  inserted into `emit_struct`'s own output right after the fields the
  *root* class declares (`Expr`'s own `span`, so every `Expr`
  descendant's struct is `{ Span *span; long __lucid_tag; <its own
  fields> }`) — the same relative position in every descendant, via
  `inherited_fields` already prepending the root's own fields first, so
  it's readable through a pointer of the *root's* own type regardless of
  which concrete subclass the pointer actually points to, under C11
  6.5.2.3p6's "common initial sequence" rule (the same reasoning `.span`
  access through an `Expr *` already relies on — see the inheritance
  entry above). `emit_constructor` sets it unconditionally to this
  class's own `class_tag` — never a real Construct argument, the same
  way `check_construct` never counts it. `codegen_sealed_match`
  evaluates the subject once into a `<root> *` temp, then an if/else-if
  chain (never a C `switch`, for the identical "break inside an arm has
  to mean whatever loop encloses the match" reason `codegen_match`'s own
  union case already documents) checking `temp->__lucid_tag ==
  <class_tag>`; a concrete-class arm's own alias, if any, is bound to an
  explicit downcast (`(NumberNode *) temp`) *inside* that arm's own tag
  check — sound only because the tag check just confirmed it, the same
  way an unchecked C union access never would be. A subtype value also
  now gets the explicit cast it needs (`Binary *` and `Expr *` are
  unrelated struct types here) at two more call sites that weren't
  covered before — a plain function call and a method call, both
  through a new shared `cast_if_needed` helper (`list[T].append(...)`'s
  own element argument gets it too).
  A `return` whose own value's static type is a union that's a strict
  *subset* of the enclosing function's own declared union return type
  (checker.lucid's own `class_assignable_to` now accepts this — see its
  entry above) is a real C type mismatch otherwise: the two unions are
  different tagged structs (a different member set means a different
  layout and different tag values), so simply returning the narrower
  value as-is — what this codegen already does for the "exactly the
  same union" case (`return self.next_token()` from inside
  `next_token`, see the Design notes entry on that bug for the full
  story) — would type-check under neither. `codegen_union_narrowing_return`
  evaluates the narrower value into its own temp once, then an
  if/else-if chain on *its own* tag re-wraps whichever member is active
  into the wider union's own tag/field layout (`find_union_member`/
  `wrap_union_value`, the same helpers an ordinary member-to-union wrap
  already uses) and returns that directly, taking over emitting the
  `return` statement itself instead of codegen_stmt's own Return arm
  doing it the usual single-expression way.
  Lucid has no subprocess/exec builtin, so codegen stops
  at emitting C text — invoking a system C compiler on it is necessarily
  a driver step outside Lucid, the same role
  `compiler/crates/lucid-codegen` itself plays (it also just shells out
  to `gcc`, from Rust rather than from the Lucid it compiles).
  `?` hoisted out of a `Call`/`Construct` argument or a `Binary` operand
  (see checker.lucid's own entry above) gets a matching
  `codegen_hoisted_prelude`, the same temp/tag-check/early-return shape
  `codegen_propagate_exprstmt` already builds for a bare `expr?`
  statement, plus one more plain C declaration unwrapping the ok value
  into the hoisted temp (a real local, declared mid-block — C99/C11
  allows this, and every other propagate temp in this file already
  relies on it). `codegen_stmt`'s Assignment/Return/ExprStmt arms all
  call `hoist_propagate` themselves (via `ast.lucid`, not duplicating
  the pattern-matching in three places) before falling back to plain
  `codegen_expr`, and codegen the *substituted* expression tree
  afterward — again needing no changes to `codegen_expr` itself, since
  the hoisted temp reads back as an ordinary `Ident`.
  `int(...)`/`float(...)` compile straight to `atol(...)`/`strtod(...,
  NULL)` (both already available — `<stdlib.h>` is already included for
  `malloc`/`realloc`); `FloatType` is a C `double`, printed with `%g`.

  Compiling `self-host/parser.lucid` itself all the way through this
  pipeline — not just past `checker.lucid`, but through `codegen.lucid`
  and a real `gcc` invocation on the result — surfaced five more real,
  previously-undiscovered bugs, none related to any feature added this
  branch, each a shape no earlier compiled program happened to exercise:
  - **`infer_local_types`'s match-arm pre-pass doesn't narrow the
    subject alias.** `codegen_sealed_match_arm`/`codegen_match_arm` both
    correctly narrow a match's own subject alias (`match e as result:
    case Call: ...` binds `result: Call` *inside* that arm, for actual
    codegen) — but the separate `infer_local_types`/`collect_local_names`
    pre-pass that hoists every local's own C declaration recurses into
    each arm's body using the *unnarrowed* `known`, so a fresh local
    assigned from the narrowed alias's own field (`a = result.args[i]`,
    once inside `ast.lucid`'s own new `hoist_propagate`) silently fell
    back to `long` instead of the real pointer type, miscompiling every
    `->` access after it. Not fixed in the shared pre-pass (used by
    every self-hosted file, too wide a blast radius to change without a
    much larger regression sweep than this branch's actual need
    justifies) — worked around locally, in `hoist_propagate` itself, by
    never naming the narrowed field read as its own local at all
    (indexing `result.args[i]` again each time instead).
  - **`codegen_propagate_assignment` never wrapped into an established
    wider type.** `value: Expr | none = none` then, later, `value =
    self.parse_expr()?` (`self-host/parser.lucid`'s own
    `parse_statement`) needs the unwrapped ok value wrapped into the
    *already-established* `Expr | none`, the same way the ordinary
    (non-`?`) Assignment arm already handles a narrower value assigned
    into an established union or sealed-base class — but the `?`
    desugaring's own unwrap always assigned the bare ok value straight
    into the target name, regardless of the name's own established type,
    a real C type error (`Expr *` into a `lucid_union_Expr_none`
    struct). Fixed by giving `codegen_propagate_assignment` the same
    `known.get(name, none)`-driven wrap/cast logic the plain Assignment
    arm already has.
  - **`collect_union_types` never scanned a local variable's own
    declared type.** It scans every registered function/method return
    type and every class field (see its own header comment, itself
    fixed for the field case earlier in this project) — but never a
    `VarDef`'s own declared type, a third union-usage site
    (`default: Item | none = none`, `self-host/parser.lucid`'s own
    `parse_class_member`/`parse_raw_function`) that's neither. Left
    `lucid_union_Item_none` undeclared the moment a union was used only
    this way. Fixed with a new `collect_var_union_types`, walking every
    `FunctionDef`/method body and the module's own top-level statements
    for a `VarDef`'s own union-typed annotation, merged into the same
    list `collect_union_types` already builds.
  - **A `VarDef`'s own initializer never got the sealed-base subtype
    cast.** `exception_type: TypeExpr = NamedType(...)` and
    `pattern: Pattern = IdentPattern(...)` (both real shapes in
    `self-host/parser.lucid`) declare a plain sealed-base class type
    with an initializer whose own static type is a narrower concrete
    subclass — the same cast an ordinary *reassignment*, a `Construct`
    argument, a `Return`, and an Attribute-assignment all already get,
    but `VarDef`'s own codegen never gave its initializer the same
    treatment (only a declared *union* type's initializer was covered).
    Fixed by adding the matching `case ClassType:` branch alongside the
    existing `case UnionLType:` one.
  - **A Lucid identifier that collides with a C reserved word compiles
    to invalid C.** `default: Expr | none = none` (`self-host/
    parser.lucid`'s own `parse_field_member`/`parse_param_list` — a
    field/parameter's own *default value* expression, named the same as
    the concept it holds) emitted a bare C local literally named
    `default`, a C keyword — `int`, `float`, and every other C reserved
    word Lucid doesn't reserve itself are the same risk, just not yet
    hit by name. Fixed with a new `c_safe_ident` (appends `_` to any of
    the ~35 C keywords Lucid doesn't already reserve as its own), applied
    at every point a Lucid-sourced local/parameter/match-alias name
    becomes a raw C identifier — `ident_target_name`, `ident_pattern_name`,
    `codegen_expr`'s own `Ident` case, `infer_expr_type`'s own `Ident`
    case, `c_param_list`, `codegen_method_param_list`, and both match-arm
    alias bindings — so a declaration and every later read of the same
    name always agree on the same escaped spelling.

  Each of the five was root-caused with a from-scratch, minimal
  reproduction, then verified the usual way (compiled through the full
  pipeline, `gcc -Wall`-compiled, diffed byte-for-byte against `lucid
  run` on the same source) before being folded into
  `compile_demo_programs/parser_gaps.lucid`, the one program in this
  branch that exercises the whole set together.
- `compile_demo.lucid` — **an actual compiler, written in Lucid,
  compiling real Lucid programs to native executables.**
  `examples/vectors.lucid` — classes, `dispatch def` operator
  overloading, multi-argument mixed-type `print` — was never written for
  this pipeline; lexed, parsed, checked, and compiled to C entirely
  through `lexer.lucid`, `parser.lucid`, `checker.lucid`, and
  `codegen.lucid`. Also compiles seventeen hand-written programs —
  sixteen real files under `self-host/compile_demo_programs/`, plus
  `imports_demo.lucid`, which lives directly under `self-host/` instead
  (see compile_demo.lucid's own header comment for why: its
  `from .lexer import ...` has to resolve to `self-host/lexer.lucid`
  under both the reference interpreter's real relative-import
  resolution and this subset's own simplified one) — (recursion,
  iteration, `%`/`and`/`or`/comparisons, `//`/`%`'s exact Euclidean
  semantics on all four sign combinations, `bools` — bool literals,
  comparisons, and `and`/`or` results printed as `true`/`false` —
  `loops` — `for` over both a list literal and `range(...)`, a nested
  `for` whose inner loop's `if_broken` clause re-breaks the outer loop,
  a `while` with its own `if_broken`, and the two range-codegen
  miscompile cases described above — `records` — printing a class
  value, alone, mixed with other `print` arguments, and with more than
  one field of mixed str/int/bool type — `strings` — `==`/`!=`/
  `<`/`<=`/`>`/`>=`, `len()`, and `+` concatenation on `str`, standalone,
  assigned to a variable, and through user functions, plus `s[i]`
  indexing, negative indices, and indexing inside a loop — `lists`
  — a `list[int]` built with a typed empty literal and `append`,
  indexed (including negatively), `len()`'d, iterated with `for`, a
  non-empty list literal iterated directly, a function taking and
  returning `list[int]`, the aliasing case (`b = a; b.append(3)`
  changes `len(a)` too, verified against the interpreter first), and
  `list[bool]`/`list[str]` — `mutation` — attribute assignment
  (`p.x = 2`), a `-> none` function that mutates a class argument's
  field in place and is called only for effect, and reading the
  mutated field back afterward, proving the caller's own object
  changed, not a copy — `methods` — a method reading and mutating
  `self`'s own fields, one method calling another method on the same
  `self`, a method taking a parameter beyond `self`, and `str`/`bool`
  method return types — `dicts` — a module-level `dict[str, str]`
  literal and `.get()`, the same inside a function body (a fresh dict
  built each call, the actual shape `self-host/lexer.lucid`'s own
  `keywords`/`single` tables use), and an empty `dict[str, int]`
  literal with a declared type — and `module_scope` — a module-level
  `int` read by a plain function, a module-level `dict[str, str]` read
  by a class method (the same `keywords.get(...)` shape
  `self-host/lexer.lucid` itself needs), and a module-level `int`
  shadowed by a same-named local assignment inside a function, which
  must read back the original module-level value afterward, not the
  local) — and `union_types` — a method returning `Item | none`
  (mirroring `self-host/lexer.lucid`'s own `handle_indentation`),
  another returning `Item | ItemError` narrowed by a `match` that also
  mutates `self` in one arm (mirroring `next_token`), two chained `?`
  propagations, one of them through a `list[Item] | ItemError` return
  (mirroring `tokenize`'s own `list[Token] | LexError`), an empty
  list literal as a constructor argument inferring its element type
  from the target field's own declared type (`Stack([], 0)`), a local
  declared as `Item | none` reassigned to a narrower `Item` and back to
  `none` across a loop (mirroring `self-host/parser.lucid`'s own
  `value: Expr | none = none` pattern, the next target this uncovered —
  see Status above), and a bare negative literal passed directly to
  `print()` (catching a real, previously-undiscovered miscompile — see
  the Design notes entry below) — this
  file's own shapes are deliberately the exact ones
  `self-host/lexer.lucid` itself needs, verified by actually compiling
  `self-host/lexer.lucid` standalone afterward: zero check errors, and
  the generated C compiles cleanly with `gcc` (see the Design notes
  entry below for the two real bugs this surfaced) — and
  `imports_demo` — `from .lexer import is_digit, is_alpha`, calling
  both, the exact cross-file shape `self-host/parser.lucid` needs
  (`from .lexer import Token, LexError, tokenize`), exercising both
  `resolve_all_declarations`' registration (checker.lucid's entry
  above) and its full-file inlining (codegen.lucid's own entry) —
  verified the generated C (about 40KB, inlining all of
  `self-host/lexer.lucid`, not just the two imported functions) compiles
  and matches the reference interpreter exactly — and
  `inheritance` — a class extending a sealed base (`NumberNode(Node)`),
  its own Construct call taking the parent's field first, a function
  returning a subtype where the declared return type is the base class
  and another where it's a union containing the base, a `match` on that
  union narrowing to the base class (not the concrete subtype), a
  union-typed field constructed/read/reassigned, `?` as a `return`
  statement's own value (on the success path only — see the Design
  notes entry below for why the failure path couldn't be included), `?`
  as a bare expression statement on *both* the success and failure path
  (the reference interpreter's own bug is specific to the
  `return`-statement shape, verified directly before relying on it), a
  `match` on the sealed hierarchy itself narrowing to a *concrete*
  subclass (`case NumberNode: ...`), a subtype value passed to a plain
  function and to `list[T].append(...)`, and a `return` whose own union
  return type is a strict subset of the enclosing function's wider one
  — and `parser_gaps` — `?` hoisted out of a `list[int].append(...)`
  argument and out of a `Binary` `+` operand, both on the success and
  failure path, `int(...)`/`float(...)` conversions and a `float`-typed
  class field, a local named `default` (a C reserved word), a `VarDef`
  declaring a sealed-base type initialized with a subtype `Construct`
  then reassigned to a different subtype, and a local declared with a
  union type that's neither a return type nor a field — the five real
  bugs `self-host/parser.lucid`'s own first full compile found (see the
  `codegen.lucid` entry above), exercised together in the one program
  — plus one
  Lucid string literal that's supposed to fail checking, proving a
  real error stops codegen
  instead of emitting broken C. `shapes.lucid` from `examples/` isn't
  compiled here (yet) — it needs `freeze()`, rejected outright in this
  subset rather than emulated: lucid-runtime's `freeze` mutates a
  shared `is_frozen` flag on a reference-counted object, and the
  reference native backend tracks the same thing at runtime on a boxed
  value, but this subset's classes carry no such flag at all, so
  neither the interpreter's nor the reference compiler's actual
  `freeze` semantics has anything to attach to here.
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
  `self-host/codegen.lucid` shares this exact gap (class field names,
  parameter names, and local variable names all become C names
  unescaped) — not fixed there either, for the same reason.
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
- **A method call sometimes reports the wrong arity — root-caused.**
  First hit as `codegen.lucid`'s `self.codegen_struct(cls)` (one
  argument beyond `self`) rejected with "method accepts fewer arguments
  than supplied"; worked around at the time by moving the logic to a
  free function (`emit_struct(cls: ClassSig)`), without knowing why.
  Hit again while adding class methods (`Checker.check_method`,
  `Codegen.codegen_method_param_list`, both taking a `ClassSig` as one
  of two parameters beyond `self`), fixed the same way, which finally
  gave enough data points to isolate it with a two-class, five-line
  repro (`class Holder: def show(self, cls: Foo) -> str: ...`, called
  with exactly the declared number of arguments, still rejected as
  having "fewer arguments than supplied") — and, crucially, that
  renaming the second parameter from `cls` to anything else made the
  repro pass. The bug is in `compiler/crates/lucid-checker/src/lib.rs`,
  not in anything self-hosted: eight call sites filter a method's
  parameter list by `!matches!(param.name.as_str(), "self" | "cls")`
  before counting/typing them for the method's registered signature —
  intended to strip the implicit receiver (`self` for an instance
  method, `cls` for a `classmethod`, per docs/class-members.md), but
  applied as a name match across the *whole* parameter list rather than
  a position check on *only* the first parameter. An ordinary instance
  method with a *later* parameter that happens to be named `cls` (nothing
  to do with `classmethod`) gets that parameter silently dropped from
  its own registered arity, undercounting it — a real bug affecting any
  Lucid program, not a self-hosting-specific gap, confirmed but not
  fixed here (out of scope for this branch; flagged separately). This
  self-hosted checker/codegen never hits it, since every method
  parameter named `cls` in `self-host/*.lucid` belongs to a *free*
  function, not a method — the bug is specific to a *method's* filtered
  parameter list, and free functions never go through that filter.
- **A real, previously-undiscovered miscompile in `codegen.lucid`
  itself: `infer_expr_type`'s `ListExpr` case returned the bare
  *element* type instead of the list's own `ListType`.** Found while
  adding `dict[str, V]` support and testing `xs = [1, 2, 3]` with no
  declared annotation (every earlier test either declared `list[T]`
  explicitly, or only used a non-empty list literal inside `for`,
  which has its own separate, already-correct
  `infer_for_iterable_element_type`): `xs` hoisted as `long` instead of
  `lucid_list_int *`, and `xs.append(4)` compiled to a bare
  `0 /* unsupported method call */;`. `check_expr`'s own `ListExpr`
  case in `checker.lucid` already got this right (`return
  ListType(et)`); `codegen.lucid`'s mirror of it didn't, most likely
  copied from `infer_for_iterable_element_type`'s different, correct-
  for-*that*-caller convention without noticing the two functions need
  different return shapes. Fixed by wrapping in `ListType`, and fixed
  the two call sites that had (correctly, for the old broken
  contract) unwrapped it back out (`codegen_expr`'s own `ListExpr`
  case, which now reads `.element_type` off the wrapped result instead
  of using it directly) — the same shape of fix `dict[str, V]`'s new
  `DictExpr` case needed from the start, once this was caught.
- **`lucid run` silently echoes the module's last top-level statement's
  value, if it isn't `none` — a REPL feature, not a language or
  compiled-binary behavior.** `compiler/crates/lucid-cli/src/main.rs`'s
  `run_file` prints `eval_module`'s own return value (whatever the last
  top-level statement evaluated to) after running the program, but only
  in the *interpreted* `lucid run` path — `lucid run --native` and
  `compiler/crates/lucid-codegen`'s own `build` just run the compiled
  binary directly, with no such echo, and this self-hosted `codegen.lucid`
  correctly doesn't replicate it either. Caught testing a throwaway
  program ending in a bare `ok()` call (an `int`-returning function) at
  top level: `lucid run` printed the function's own `print()` output
  *plus* a second, unexpected line echoing `ok()`'s return value — not
  a self-hosted codegen bug, a mismatch between the driver's REPL-style
  convenience and what a "real" program (or this subset's own compiled
  output) actually does. Every hand-written
  `self-host/compile_demo_programs/*.lucid` file happens to end in a
  `print(...)` call or a loop (both `none`-typed), so this was never
  actually a hazard for the existing differential tests — worth keeping
  that pattern for any new one added, since ending a compile_demo
  program in a bare non-`none` top-level expression would make its own
  `lucid run` reference output diverge from the compiled binary's for a
  reason that has nothing to do with the compiled program being wrong.
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
- **A `match ... as name:` binding only lives inside its own case body —
  not for the rest of the enclosing function, even past the whole
  `match` statement.** Written naturally at first as `match subject_type
  as st: case UnionLType: pass / case LType: ...error...(...)` followed,
  *after* the match statement ends, by more code reading `st` — this
  compiles (self-hosted files are ordinary Lucid, no different from any
  other program), but `lucid run` rejects it with "undefined variable
  'st'" the moment execution reaches that later code, since `st` was
  never actually bound there. Not a bug found by inspection — found by
  actually running `checker.lucid` (this file is Lucid, checked and run
  by the real toolchain like any other) and hitting the error directly
  while adding `check_match`. The fix moved everything that needed `st`
  *inside* the `case UnionLType:` arm itself, matching how every other
  narrowed binding in this codebase is already scoped in this codebase's
  own style (see the existing "Reassigning a `match` statement's own
  scrutinee inside an arm" note above for the same underlying rule from
  the other direction). Worth remembering for any future self-hosted
  file: a `match ... as name` binding is arm-scoped, full stop — needing
  the narrowed value across multiple arms or after the match means
  either restructuring around a single arm's own block, or assigning a
  plain, pre-declared variable inside each arm instead (which *is* an
  ordinary local, not the match's own narrowed binding — `codegen.lucid`'s
  `codegen_propagate_assignment` hit the identical shape with a plain
  `wrapped_err` variable first assigned inside two different `match`
  arms, fixed the same way: pre-declare it with a default value before
  the match, then only ever *reassign* it inside an arm).
- **A real, previously-undiscovered miscompile in `codegen.lucid`'s new
  union-return-type wrapping: a function returning its own recursive
  call got double-wrapped.** `codegen_stmt`'s Return arm, when the
  enclosing function's declared return type is a union, wraps the
  returned value into that union's tagged struct (`wrap_union_value`) —
  correct for `return LexError(...)` or `return self.make(...)`, where
  the value's own static type is one *member* of the union. But
  `self-host/lexer.lucid`'s `next_token` also has `return
  self.next_token()` (called again after re-lexing past a line
  continuation) — a value whose own static type is *already* the exact
  same union (`Token | LexError`), not a member of it. Wrapping it again
  produced `(lucid_union_LexError_Token){.tag = 0, .v0 =
  lucid_method_Lexer_next_token(self)}`, assigning a
  `lucid_union_LexError_Token` value into a field typed `LexError *` — a
  real `gcc` compile error (`incompatible types when initializing type
  'LexError *' using type 'lucid_union_LexError_Token'`), not a silent
  miscompile, but still a program `checker.lucid` accepted (correctly —
  its own `assignable_to` already treats "value's type equals the whole
  union" as valid, via `types_equal`'s own first branch) that codegen
  couldn't actually compile. Caught immediately on the first attempt to
  compile `self-host/lexer.lucid` itself through this pipeline — no
  hand-written `compile_demo_programs/*.lucid` file happened to return
  its own recursive call, so nothing exercised this path before. Fixed
  by checking the returned expression's own inferred type first: if
  it's already the exact declared union (an `UnionLType`, matched before
  falling through to the general member-wrapping case), return it
  as-is.
- **`print(-1)` printed a garbage value instead of `-1` — a real,
  previously-undiscovered miscompile, unrelated to union types, caught
  while extending `union_types.lucid` to exercise a bare negative
  literal as a `print()` argument for the first time.** A C integer
  literal like `-1` defaults to type `int`, not `long`, unless it's too
  large to fit — but `codegen_print`'s format string always uses `%ld`
  for a non-str/bool argument, and `printf` is variadic: its argument
  promotion only ever widens `int` to `int` (already done), never on to
  `long`, so `printf("%ld\n", (-1))` reads a `long`-sized value out of
  storage that only ever held an `int`-sized one — undefined behavior,
  and on this machine, a real wrong answer (`4294967295`, `-1`
  reinterpreted as an unsigned 32-bit value, not `-1`). Every earlier
  `compile_demo_programs/*.lucid` file only ever printed a *computed*
  int value (an arithmetic result, a variable, a function's return
  value) — always genuinely `long`-typed by the time it reaches
  `printf`, since it flows through a `long`-typed C local or parameter
  along the way — never a bare negative literal passed straight through
  as a `print()` argument, so nothing exercised this exact path before.
  Fixed with an explicit `(long)` cast on every non-str/bool `print()`
  argument (`codegen_print` and `codegen_print_with_class_args` both) —
  correct whether or not the underlying C expression was already
  `long`, and cheap enough to apply unconditionally rather than trying
  to detect which expressions specifically need it.
- **A real bug in `compiler/crates/lucid-checker` itself: a class used
  only as a `dict[K, V]` value type, forward-referenced from *inside*
  another class's own field, breaks type-checking for an unrelated call
  between two *different* functions.** Found writing `is_subclass_of`/
  `class_assignable_to` in `types.lucid`: both take `classes: dict[str,
  ClassSig]`, and `class_assignable_to` calling `is_subclass_of(classes,
  ...)` failed with `argument 1 to 'is_subclass_of' has incompatible
  type` — pointing at `classes` itself, even though both functions
  declare the exact same parameter type. Root-caused with a from-scratch
  ~25-line reproduction, independent of this codebase: a class
  `ClassSig` with a field `methods: dict[str, FuncSig]`, where `FuncSig`
  is a plain class defined *later* in the same file (`ClassSig` forward-
  references it, ordinary and normally harmless in this language) — the
  moment `dict[str, ClassSig]` is passed from one function to a
  *different* function also declaring `dict[str, ClassSig]` as its own
  parameter, the second function's own call into the first stops
  type-checking, with the exact same "incompatible type" error, on the
  exact same parameter. Confirmed by elimination: removing `class
  FuncSig` from the file entirely, or moving its definition *before*
  `ClassSig` instead of after (eliminating the forward reference),
  both independently make the error disappear. `types.lucid` genuinely
  needs `ClassSig.methods: dict[str, FuncSig]` (unrelated to
  `class_assignable_to`, pre-existing since methods were first added)
  and needed `is_subclass_of(classes: dict[str, ClassSig], ...)` called
  from a second function with the identical parameter type (new, for
  class inheritance) — the two together are what triggers it. Not fixed
  in the Rust source, out of scope for this branch; worked around by
  moving `class FuncSig`'s own definition ahead of `class ClassSig`'s in
  `types.lucid`, breaking the forward reference that triggers the bug,
  with no change to either class's own fields.
- **A real bug in `compiler/crates/lucid-runtime` itself: `return
  expr?` crashes the reference interpreter whenever `?` actually
  propagates an error — the success path works fine.** Found trying to
  differentially verify this branch's own new return-position `?`
  support: `def use_item(ok: bool) -> Item | ItemError: return
  get_item(ok)?` ran correctly under `lucid run` when `get_item`
  succeeded, but raised `Runtime Error: return is only valid inside a
  function` (a synthetic, zero-span error — not tied to any real source
  position) the moment `get_item` actually failed. Root-caused by
  reading `Expr::Propagate`'s own evaluation in `lucid-runtime`: on the
  error path, it returns a special `Value::Return(Box::new(val))`
  sentinel meant to unwind up through the enclosing statement machinery
  as if a `return` had just executed — correct when `?` is a plain
  assignment's right-hand side (a different code path handles unwrapping
  that sentinel correctly, verified independently and used throughout
  this branch's own `union_types.lucid`), but when `?` is instead the
  *direct value of an actual `return` statement*, that statement's own
  evaluator receives the sentinel as its "value" and wraps it in
  *another* `Value::Return(...)`, doubly nested — something downstream
  doesn't expect. This subset's own `codegen.lucid` doesn't share the
  bug: its desugaring (`codegen_propagate_return`) is a plain C
  statement sequence (a temp, an `if` with an early `return`, then a
  final `return`), with no sentinel value or double-wrapping possible in
  C's own control flow — confirmed correct by direct inspection of the
  generated C and by structural identity with the assignment-position
  desugaring's own error-path handling, already differentially verified.
  But it meant this one feature's *error* path couldn't be verified the
  usual way (a `lucid run` reference to diff a compiled binary against),
  since the reference interpreter itself can't run the program that
  would exercise it — `compile_demo_programs/inheritance.lucid`'s own
  `?`-as-return-value case is deliberately called only on the success
  path for exactly this reason. The bug is genuinely specific to the
  `return`-statement shape, not to `?`/propagation in general — verified
  directly that `?` as a bare expression statement (`self.
  consume_stmt_end()?`, added the same session once this was
  root-caused) propagates an error correctly under `lucid run` on both
  paths, with no special handling needed, so `compile_demo_programs/
  inheritance.lucid`'s own bare-statement `?` case *is* differentially
  tested on both paths. Not fixed in `lucid-runtime`, out of scope for
  this branch.
