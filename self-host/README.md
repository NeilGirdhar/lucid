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
it, not a claim that it's close to done.

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
  interpreter and successfully parses its own imports' source
  (`parser_demo.lucid` proves this — see below).
- `parser_demo.lucid` — parses a small sample program, then reads and
  parses `lexer.lucid`'s own source. The second part is the self-hosting
  proof point for this piece: Lucid source a human wrote, run by the
  reference interpreter, correctly parsing real Lucid source into a
  well-formed AST.

Run them:

```
lucid run self-host/lexer_demo.lucid
lucid run self-host/ast_demo.lucid
lucid run self-host/parser_demo.lucid
```

Self-tokenizing takes on the order of 20-30 seconds under the debug
interpreter build — `lexer.lucid` builds its output strings one character
at a time (`text = text + c`), which is quadratic in a debug build with no
inlining. Worth revisiting once there's more here to justify it (either a
StringBuilder-style pattern in the language, or building lists of
characters and joining once), but it works, which is what "start on"
called for. Self-parsing (tokenize + parse) `lexer.lucid`'s ~500 lines
takes on the order of 30 seconds for the same reason, one layer up.

None compile under `lucid run --native` yet — see "Native codegen
gaps" below. Both run correctly under the reference interpreter, which is
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
