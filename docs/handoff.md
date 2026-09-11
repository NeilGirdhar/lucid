# Implementation handoff

This handoff describes the current state of the Lucid implementation and the
work that remains before it can claim complete conformance with the
specification. The repository is an active prototype: the specification is
the source of truth, while the compiler crates provide an increasingly broad
executable subset.

## Current status

The workspace contains a working lexer, parser, checker, runtime, native
code generator, and command-line test runner. The implementation covers a
substantial portion of the documented surface, including classes and traits,
inheritance, factories and class methods, getters and setters, generics with
variance and mutability views, multiple dispatch, recoverable results and
`?` propagation, `match`, context managers, decorators, modules, collection
operations, argument bundles and spread, closures, async futures, reflection,
custom protocols, UTF-8 strings, arbitrary-size integers, complex numbers,
native imports, and several builtins.

These features are tested through the existing Rust unit tests and the
specification examples. They should be treated as implemented behavior of the
current prototype, not as proof that every rule in the specification is
complete. In particular, the current evaluator and native emitter still make
semantic decisions directly from the AST. The target architecture in
[Implementation architecture](architecture.md) requires one typed control-flow
IR shared by both execution paths.

Reflective writes now follow the same setter path as ordinary attribute
assignment. Static receivers, inherited setters, and erased `Any`/union
receivers convert the supplied value to the setter parameter type and invoke
the generated setter before falling back to a stored field.
They also enforce the frozen-object mutation rule when no setter is present,
so `setattr` cannot bypass immutability.
The interpreter now mirrors native reflection for inherited getters: `getattr`
evaluates the getter and `hasattr` reports it as present.
Native `getattr` also honors its optional default for static objects, erased
values, and missing attributes, matching the interpreter's three-argument
form.
Reflection builtins no longer require a literal attribute name in native
codegen; computed strings use the same checked dynamic dispatch as erased
values.
Native reflection preserves receiver-before-default evaluation order when a
missing static attribute supplies a fallback.
`hasattr` also reports ordinary and inherited methods, matching the runtime
object member set.
Computed reflection arguments are materialized in source order before the
dynamic helper runs, so receiver, name, value, and fallback side effects do
not depend on C argument-order rules.
Static reflective writes also materialize typed receivers once before frozen
checks and setter dispatch.
Native `round` now enforces the same one-or-two-argument arity as the
interpreter instead of silently discarding extras.
Native `abs` likewise rejects extra or missing arguments before lowering.
Native `round` validates that `ndigits` is an integer, and native `int` uses
strict whole-string parsing for string literals instead of accepting a valid
prefix followed by junk.
Native `float` now applies the same strict whole-string parsing to string
inputs.
Native `int` preserves arbitrary-size integer literals instead of narrowing
them through a signed 64-bit conversion.
Native `int` also preserves arbitrary-size integers that cross an erased
`Any` value, while retaining strict parsing for dynamic string inputs.
String inputs that overflow the machine integer range now promote to a native
BigInt as they do in the interpreter, including statically known strings.
Float-to-int conversion now matches Rust's saturating cast semantics for
infinities and maps NaN to zero instead of relying on undefined C casts.
The one-argument native `round` path uses the same guarded conversion for
float infinities and NaN.
Native and interpreter `abs` preserve the `int.nan` and `-int.inf` sentinel
values instead of overflowing on their reserved machine representations.
Native `round` now rejects BigInt values that arrive through erased `Any`,
matching the interpreter's numeric capability dispatch.
Native `str` now renders complex values in the same `(real+imagj)` form as
the interpreter.
Native `complex` now rejects non-numeric values after erasure instead of
silently treating them as zero.
Native `chr` now requires an integer runtime value after erasure, matching the
interpreter instead of coercing floats and strings.
Dynamic `int` and `float` conversions now reject unsupported `none` and object
values instead of silently producing zero.

## Verified baseline

The last full workspace run completed successfully in both debug and release
profiles:

```text
cargo test --workspace --all-targets --quiet
69 checker tests
65 CIR tests
4 shared-ABI tests
34 CLI/CIR integration tests
1 CLI unit test
31 CLI/spec tests
308 native-codegen tests
150 runtime tests
20 syntax tests
50 compiler-database tests
17 configuration tests
749 tests passed
```

The same 749 tests also pass with:

```bash
cargo test --workspace --all-targets --release --quiet
```

The workspace also has no Rust doctests currently registered; the explicit
`cargo test --workspace --doc --quiet` pass completes successfully.

The feature-complete matrix also passes with:

```bash
cargo test --workspace --all-features --all-targets --quiet
```

The corresponding all-features clippy gate also passes with `-D warnings`.
The same all-features matrix passes in release mode as well.

The documentation site also builds without warnings:

```text
uv run zensical build --clean --strict
No issues found
```

`git diff --check` is clean. The implementation is backed up on the active
development branch. A repository-wide `cargo fmt --check` currently reports
large formatting differences in the existing codebase, so do not apply a
whole-tree formatter as part of an unrelated change.

## Recent implementation work

The most recent changes tightened behavior where the two execution paths had
diverged:

* Truthiness now agrees across the interpreter and native backend for empty
  lists, dictionaries, sets, strings, and ranges, as well as zero BigInts and
  zero complex values. Native typed collection and string conditions use
  single-evaluation helpers, so checking a value cannot duplicate calls or
  other side effects. Tagged object values retain their class-specific
  `__bool__`/`__len__` callback when they cross an `Any` boundary, with
  coverage for both protocol forms.
  Callback registration is limited to methods with the documented return
  types, preventing malformed protocol declarations from creating an ABI
  mismatch in generated C.
  Native object metadata grows on demand instead of silently dropping
  truthiness and freezing information after a fixed object-count limit, with
  checked capacity growth at the allocation boundary.
  Native object metadata also records each class ancestor, so a value erased
  to `Any` still satisfies `is` checks against its base classes. The registry
  scans all entries for a pointer rather than stopping at the concrete-class
  entry; this preserves inheritance semantics without adding a second object
  representation.
  The interpreter now applies the same ancestry rule to `is`, `is not`, and
  class-pattern matching, keeping erased and statically known checks aligned
  across both execution paths.
  Native ancestry collection now distinguishes the one class parent from
  trait bases and resolves it after collecting all class declarations. Trait
  order and forward class declarations therefore cannot create invalid
  `_freeze` aliases or corrupt inherited field layout.
  The same metadata pass now unwraps `export` declarations, so exported
  classes participate in native inheritance, construction, and type checks
  exactly like local declarations.
  Checker member collection no longer treats the first base as a class
  parent; class-parent resolution remains based on declared class types, so a
  trait listed first cannot hide a later class parent.
  Runtime and native hierarchy walks now track visited classes, preventing a
  malformed cyclic inheritance graph from recursing or hanging while checking
  ancestry, pattern matches, field layout, or dynamic type aliases.
  The checker now resolves class parents after declaration collection and
  rejects cyclic inheritance with a source diagnostic before later passes.
  Native inheritance coverage now also exercises a child declared before its
  base, including inherited construction fields and dynamic type checks.
  Native object-tag growth now checks both counter and allocation-size
  overflow before calling `realloc`, preserving a recoverable process failure
  instead of allowing wrapped allocation sizes.
  Native union-returning calls now support dynamic field and getter reads
  through a checked object-attribute dispatcher, so `value = fallible()?` can
  be inspected after propagation without treating the `LucidVal` wrapper as a
  C struct.
  Instance methods with positional arguments on those union values now
  dispatch through the same concrete-class registry, converting arguments to
  each selected method's declared native type before the call.
  Reflective `getattr` reads on those union values use the same dispatcher,
  rather than assuming a statically known struct layout.
  `hasattr` now performs a non-throwing runtime field/getter presence check
  for the same erased objects.
  Reflective `setattr` writes on erased objects now dispatch through generated
  field metadata, enforce frozen-object checks, and convert the assigned
  `LucidVal` to the declared field representation.
  Static `getattr`/`hasattr` calls now recognize getters as well as stored
  fields, matching ordinary attribute lookup.
  The checker now rejects `?` at module scope, where no enclosing function
  return type can accept the propagated error; this prevents native `return`
  generation from producing invalid top-level C control flow.
* The interpreter's supported-iterator dispatch now reports a recoverable
  non-iterable error for malformed intermediate states instead of retaining an
  internal panic path.
* Native context-manager lowering and dynamic operation selection now return
  explicit code-generation errors for malformed internal shapes instead of
  relying on `unreachable!()` assertions.
* CIR counted-loop recognizers now reject malformed update and literal shapes
  through their normal unsupported-result path instead of panicking while
  constructing a control-flow graph.
* Database typed-HIR match collection now reports malformed match results as a
  lowering error instead of unwrapping an intermediate option.
  Static `elif` extraction follows the same checked path when selecting a
  compile-time branch.
* CIR short-circuit lowering now rejects impossible operator shapes through
  its normal unsupported-expression result instead of panicking.
* CIR verification and shape/type lowering now report missing blocks or
  malformed intermediate shapes through checked results rather than internal
  `expect`/`unreachable!()` paths.
* Parameterized native result adapters reject signatures above the fixed
  sixteen-argument ABI limit with `InvalidOperation` before entering unsafe
  legacy calls, including pointer-output FFI adapters.
* Cranelift power lowering now reports a missing constant exponent as an
  explicit unsupported-instruction error instead of relying on an internal
  assertion.
* Cranelift instruction-result extraction now returns a checked optional
  result, rejecting unknown instruction forms instead of panicking.
* CIR execution now exposes an `UnsupportedInstruction` error for malformed
  arithmetic instruction variants; the shared ABI maps it to `InvalidOperation`.
* Project diagnostics now preserve already-collected errors if a repeated
  parse query fails unexpectedly, rather than unwrapping its result.
* Checker subtype and numeric-special-value paths now return ordinary
  negatives for malformed intermediate types instead of relying on internal
  `unreachable!()` assumptions.
  Unknown type-position names now produce a typed diagnostic rather than an
  internal assertion.
* Logical `and` and `or` now short-circuit while honoring user-defined
  truthiness through `__bool__` and `__len__`.
* BigInt inversion follows `~n = -n - 1` for arbitrary values, and `sum`
  handles BigInt, floating-point values, and an optional start value.
* Capability checks distinguish an absent trait or protocol from a present
  one that is not callable.
* Collection constructors, `extend`, `join`, `any`, and `all` accept the
  documented iterable forms, including dictionary keys where the spec calls
  for them.
* Runtime `for` loops iterate dictionary keys and consume user iterators
  lazily, so `break` stops iteration before later `next()` calls and side
  effects.
* Runtime loops and comprehensions now reject raw strings as iterables, matching
  the checker and the specification; callers use the immutable `str.chars`
  view for character iteration.
* `any`, `all`, `iter`, `enumerate`, and `reversed` apply the same raw-string
  restriction; explicitly string-friendly helpers such as `zip` remain so.
* Loop and comprehension targets use fresh iteration environments, so closures
  retain each iteration's value and comprehension targets do not rebind outer
  names.
* Loop environments are restored on destructuring and body errors, allowing an
  enclosing handler to continue in the correct outer scope.
* Hashing frozen dictionaries sorts their keys first, so equal dictionaries
  receive equal hashes despite `HashMap` storage order.
* Set equality is order-independent, and frozen-set hashes sort element hashes
  before folding, matching set semantics and the hash contract.
* List `pop()` now uses an explicit recoverable empty-list error path instead
  of relying on an internal `unwrap`, with runtime regression coverage.
* `enumerate` no longer uses an `expect` after custom-iterator probing; a
  malformed iterator now returns a recoverable runtime error.
* BigInt-to-byte and BigInt-to-bytearray conversion uses checked narrowing,
  eliminating panic assumptions after range validation.
* Native context-manager and function emission now reports missing inferred
  local types as `CodegenError` instead of panicking on map lookups.
* The `emit-c` CLI path now runs the type checker before generating C, matching
  the validation gate already used by native builds and execution.
* CLI native-entry selection now reports an explicit lowering error if no
  compatible backend handle is available instead of panicking on an internal
  option invariant.
* `run-cir` accepts `--step-limit N` and executes through the bounded shared
  ABI path, making deterministic parameterized CIR runs available from the
  command line.
  Unbounded execution now refuses to fall back from result-ABI compilation
  when the function contains recoverable division-family operations, so a
  backend limitation cannot silently turn recoverable errors into traps.
* Native equality now compares lists, sets, and dictionaries structurally,
  keeping native behavior aligned with the interpreter instead of comparing
  container addresses.
* Native frozen-set and frozen-dictionary hashes now use deterministic ordering
  consistent with the interpreter's hash contract.
* Native `===` and `!==` use an identity helper instead of comparing C string
  addresses, while preserving heap-container identity semantics.
* Dictionary membership uses the same key representation as construction and
  indexing, so non-string keys behave consistently.
* Numeric hashing covers float and complex values, reusing the integer hash for
  exactly integral values so `1`, `1.0`, and `1 + 0j` obey the equality/hash
  contract in both backends.
* Arbitrary-precision integers outside `i64` now remain hashable through a
  stable digit-based fallback, while in-range values retain integer hashes.
* Interpreter addition, subtraction, and multiplication now promote an
  overflowing machine integer to `BigInt` instead of wrapping or panicking,
  preserving arbitrary-precision integer semantics at the operation boundary.
* Mixed `BigInt`/`float` arithmetic, comparisons, exponentiation, and `sum` now
  follow the checker’s numeric promotion rule and produce floating-point
  results, including explicit infinity for values outside the finite `float`
  range.
* Native mixed `BigInt`/`float` expressions now carry the same promotion
  through inferred variable types and never route fractional operands through
  decimal BigInt helpers; large integral-looking floats also print without an
  out-of-range `int64_t` conversion.
* The Cranelift integer ABI now supports sixteen positional parameters across
  ordinary, void, and recoverable-result entry points, with an executable
  boundary regression instead of an arbitrary eight-argument rejection.
  Parameterized result calls also expose a pointer-based output-storage API for
  FFI callers, matching the zero-argument `NativeResult` contract.
  Compiled handles expose their parameter count so callers can validate unsafe
  invocation vectors before crossing the native boundary.
  The zero-argument `call_result` adapter now reports an argument-count error
  for parameterized handles instead of asserting, keeping recoverable ABI
  misuse out of the panic path.
  Checked legacy native adapters now expose recoverable wrappers for ordinary
  value and void calls, separating argument-count errors from incompatible
  return-ABI requests without invoking assertion-based unsafe entry points.
* Typed-CIR function lowering now accepts pure local bindings before a bare
  `return`, preserving initializer instructions while emitting a verified
  void terminator; this shape no longer needs the AST linear fallback.
* The typed conditional fast path now requires the conditional to be the
  function's actual root, so an earlier statement-level `if` cannot swallow a
  later return expression.
* Dynamic statement-level conditionals with one direct return per arm now
  enter the shared typed-CIR diamond builder before the AST compatibility
  adapter, preserving the same branch and merge semantics for both backends.
  The typed API also supports a value-returning branch with a void fall-through
  and verifies both outcomes before backend lowering.
  Statement conditionals with a `pass` fall-through now use that optional
  typed builder as well, including one-sided conditionals with an implicit
  void fall-through.
  A no-`else` conditional whose `elif` guards are all statically false now
  follows the same typed optional path.
* Primitive literal `match` functions now lower through verified CIR decision
  chains: two-arm matches support expression results, while larger matches
  support literal results followed by a wildcard. More complex patterns remain
  explicit lowering work rather than being silently interpreted by a
  compatibility fallback.
  The supported two-arm shape now carries its subject, arm results, and
  literal discriminator as one typed-HIR node before CIR lowering, including
  the simple branch-local assignment-and-return form.
* Canonical counted `while` and `for range` CIR loops now accept a trailing
  `pass` after the induction update, preserving the specified no-op statement
  without widening the loop lowering shape. They also accept an
  `if_broken: pass` completion clause; effectful completion clauses remain
  outside this narrow lowering form. Range accumulators support both additive
  and subtractive induction updates with checked CIR arithmetic.
* Pure discarded expressions before a return now pass through typed-CIR
  lowering; effectful expressions remain rejected instead of being dropped.
  Their rejection now carries a dedicated diagnostic explaining that an
  effectful discarded expression needs a non-typed-CIR lowering path.
* Manifest `entry_point` lookups now enforce public internal targets even when
  callers use the API before explicitly invoking whole-manifest validation.
* The interpreter now exposes a checked `call_named` boundary for invoking
  functions and builtins by name, providing the runtime hook needed by
  manifest entry-point execution without exposing its internal environment.
  It also resolves dotted public targets through imported module values, so
  entries such as `.package.main` use the same module namespace as source code;
  the manifest-relative leading dot is accepted directly.
  It enforces the same private-name restriction as module imports, so
  underscore-prefixed functions cannot be reached through entry points.
  Empty and whitespace-containing names are rejected before lookup as well.
* `lucid run <file> --entry NAME` now evaluates the module and invokes a
  named function through that boundary. Native execution supports zero-
  argument entries and prints their return value through the shared formatter;
  entries requiring arguments or dispatch overload selection fail explicitly.
  A missing entry name is rejected explicitly instead of being ignored as a
  malformed option, and duplicate `--entry` flags are rejected rather than
  silently choosing one. The native rejection has an executable CLI regression
  as well.
  When the name is present in `project.yaml`'s `entry-points` table, the CLI
  resolves its public name to the configured dotted target before invocation.
  Unknown names are rejected when a manifest declares an entry-point table.
  Interpreted entry points enter the manifest's `library-context` through the
  existing context-manager teardown path; native runs reject that combination
  until native cleanup has a matching ABI.
* Native integer exponentiation now uses exact checked arithmetic and promotes
  overflow results through the existing BigInt runtime instead of converting
  them through lossy floating-point `pow`.
* Binary conversion builtins now reject unsupported inputs instead of silently
  producing an empty buffer or stringifying arbitrary values; bytearray item
  validation accepts only integer values in `0..255` in both backends.
* Native `del` now updates the reflected local-binding set, matching the
  interpreter's lifetime semantics until a deleted name is rebound.
* Native binding liveness is now represented at runtime, so `locals()` remains
  correct when `del` occurs inside a conditional branch or context manager.
* Runtime module reflection now preserves declaration insertion order for
  `fields(module)`, matching class reflection and native output instead of
  exposing hash-map iteration order. Deleting a binding removes it from the
  reflected set, and rebinding records its new insertion position.
* Interpreter `locals()` now excludes intrinsic builtin bindings, while still
  exposing a builtin name after user code rebinds it.
* The checker now treats `Bytes` as a distinct immutable buffer type rather
  than silently aliasing it to `str`; byte indexing is typed as `int`.
* Recovering syntax parses now preserve valid prefixes and report lexical
  failures as structured `ParseError` values with source spans, matching
  grammar-error recovery and giving diagnostic consumers one error shape.
  Prefix and radix lexing now binds validated lookahead characters directly,
  eliminating the last production lexer `unwrap()` assumptions.
* Boolean literals are now preserved as exact `LiteralBool` types, with the
  parser accepting `true` and `false` in type positions and widening them to
  `bool` when required.
* Integer literal annotations now compare the written initializer value, so
  `answer: 42 = 42` succeeds while `answer: 42 = 41` is rejected.
* Floating-point literal annotations now preserve exact written values, with
  ordinary inferred bindings widening back to `float`.
* String literal annotations now preserve exact written values and widen to
  `str` for ordinary inferred bindings.
* Literal string, float, and boolean values are normalized to their widened
  primitive types when used as receivers in ordinary operations, preserving
  exactness without breaking method and operator lookup.
* Indexed assignment now checks sequence index and element types, rejects
  mutation of immutable `Bytes`, and enforces integer elements for byte
  buffers.
* Indexed assignment now also enforces mutability views, rejecting writes
  through `~T` and `!T` receivers before container-specific checks.
* Dictionary indexed assignment uses the same canonical key representation as
  dictionary construction, lookup, and membership.
* `dict()` pair-sequence construction accepts non-string hashable keys using
  that same representation instead of rejecting them.
* `list.append` validates its exact one-argument signature in both interpreter
  and native lowering paths.
* String method arity is validated consistently: `split` accepts zero or one
  argument, and trimming/case methods accept none.
* Zero-argument collection methods (`clear`, `keys`, `values`, and `items`)
  reject unexpected arguments in the interpreter, matching native lowering.
* The `str.chars` view is frozen in both execution paths, enforcing its
  documented read-only sequence contract.
* Native frozen sets reject `remove` and `clear`, matching the interpreter's
  mutation barriers for immutable collections.
* Interpreter `from ... import` now rejects leading-underscore private names,
  matching static module visibility checks.
* Module attribute access also rejects leading-underscore names at runtime,
  preventing private members from leaking through plain module imports.
* The native project loader applies the same private-import rejection before
  flattening local modules for code generation.
* Shared ABI consumers now use one `NativeResult::from_result` conversion path
  for successful integer values and checked CIR errors.
  The shared `execute_cir` bridge now maps a valid void return to successful
  zero, matching the mixed value/void result convention used by native CFGs.
  Parameterized CIR execution is exposed through `execute_cir_with_args`, so
  the same conversion contract applies when positional arguments are present.
  Its bounded companion, `execute_cir_with_args_and_step_limit`, preserves the
  same error conversion for deterministic cyclic-control-flow execution.
  Step-limit exhaustion now has its own `StepLimitExceeded` native error code,
  distinct from unsupported-operation failures.
* Cranelift's `call_result` adapters now apply the same shared-ABI convention
  to void functions, executing them and returning successful zero through both
  value-returning and pointer-output calls.
  Parameterized adapters apply that convention to ordinary value and void
  entry points as well as dedicated recoverable-result functions.
* Native iterator and collection lowering no longer unwraps method-owner
  lookups after separate presence checks; the owner is carried through the
  checked branch, keeping impossible metadata states recoverable at codegen.
  Named argument-bundle lowering now follows the same checked path instead of
  unwrapping filtered argument names.
  Keyword-variadic calls now use that checked path as well.
  CIR argument-count failures retain a dedicated `UnexpectedArgumentCount`
  native error code instead of being collapsed into `InvalidOperation`.
  The parameterized result adapter returns that code for an arity mismatch
  instead of asserting across an FFI boundary; its pointer-output companion
  writes the same diagnostic into non-null caller storage before returning
  failure.
  Project manifest validation now searches all ancestor directories, so
  nested source modules receive the same configuration checks as root files.
  `emit-cir` and `run-cir` now use that same validation path, with an
  integration test covering an invalid manifest above a nested source file.
  The native result ABI also represents mixed value/void CFG returns as a
  successful zero-valued result on the void edge, so one-sided conditionals
  can execute natively without changing the ordinary void ABI.
  Division-family errors in a returning CFG branch now retain a block-local
  status, and direct branch returns propagate that status through NativeResult.
  Native result-ABI `+`, `-`, and `*` overflow now reports
  `ArithmeticOverflow` with a zero value instead of trapping.
  Constant-exponent native `Pow` overflow follows the same recoverable path.
  Constant negative exponents now return `NegativeExponent` through the native
  result ABI instead of being rejected during lowering.
  The CLI has an end-to-end regression proving that error reaches `run-cir`.
  Parameterized integer powers now use bounded checked native lowering, with
  negative and oversized exponents reported through the result ABI.
  Native unary negation of `i64::MIN` now reports `ArithmeticOverflow` through
  the result ABI instead of trapping.
  Native invalid shifts and overflowing left shifts now report
  `ArithmeticOverflow` through the result ABI instead of trapping.
  Multiplication's `i64::MIN * -1` edge case now preserves an earlier
  recoverable error and avoids an accidental machine divide trap.
  Multiple division-family operations in one block preserve the first
  recoverable error while later operations continue with safe sentinel values.
  Bitwise operations can now surround recoverable divisions in a straight-line
  result-ABI block without losing the first error status.
  Comparisons can likewise follow checked division in result-ABI blocks while
  preserving an earlier division error.
  Constant powers above the bounded native exponent limit now return
  `InvalidOperation` through the result ABI instead of being rejected during
  lowering.
  Direct-returning conditional branches now use a no-`Phi` form when they
  contain division-family operations, so each branch carries its own status.
  Typed-HIR conditional expressions now lower directly to CIR diamonds; arms
  with recoverable division use direct branch returns so their status survives
  native lowering without an AST fallback.
  Statement-level `if` forms whose arms return directly now become the same
  typed-HIR `if` node, so conditional-return functions do not reconstruct
  control flow from syntax.
  The CIR source lowerer now emits a real Phi-backed loop CFG for the
  canonical parameter-backed integer induction form (`while n > 0: n -= 1`),
  with bounded execution and native lowering sharing the same blocks.
  Ordinary assignment updates such as `n = n - 1` use the same loop lowering.
  Void induction functions with no explicit return now lower to the same CFG
  and terminate with a verified `Return(None)`.
  The induction recognizer also accepts `!=` conditions and lowers them to
  the same checked back-edge shape.
  It rejects empty or multi-statement bodies without indexing assumptions and
  accepts a trailing `continue` as the canonical back-edge form.
  It also accepts a preceding local integer initialization from a parameter
  or literal, preserving the initialized value in the entry block.
  Bounded `for i in range(...)` accumulation now lowers to paired index and
  accumulator Phis for one-, two-, and three-argument ranges (including
  descending constant steps), with literal or parameter-seeded assignment and
  `let` accumulator initialization. Both augmented and ordinary
  `total = total + i` updates are accepted. Loop-body HIR collection seeds the
  iteration binding without leaking it outside the loop scope.
  Canonical range loops also accept a trailing `continue`, while rejecting
  unsupported body shapes before indexing them.
  Cranelift’s recoverable `NativeResult` ABI now has a differential regression
  executing the same range-loop CFG, so this control flow is verified on both
  backends.
  Typed-HIR `and`/`or` expressions lower to lazy CFG diamonds rather than
  eager boolean instructions, preserving unchosen-arm behavior.
  Typed-HIR unary `+` now lowers as an identity instead of forcing an AST
  fallback.
  Constant typed short-circuit operands fold before lowering the dead branch,
  so recoverable errors in that branch are never executed.
  Typed-HIR identity comparisons (`is`, `is not`, and identity spellings) now
  use the same comparison instructions as the AST path.
  Statically false `while` statements are folded away by linear CIR lowering,
  preserving the surrounding statement sequence instead of rejecting it.
  Loops over statically empty list, set, dict, or string literals are folded
  away by the same path.
  The result-ABI call adapter also turns an invalid oversized argument list
  into `InvalidOperation` instead of panicking.
* The native result ABI now lowers verified straight-line integer constants
  and checked `Add`/`Sub`/`Mul` chains. A final `Div`, `FloorDiv`, or `Mod`
  preserves division-by-zero and `i64` overflow errors without a trap-based
  ABI; a chain without division returns a successful status through the same
  ABI.
  It also accepts verified non-division control flow, including jumps and
  Phi merges, and returns a successful status on every path.
* The checker enforces private-import rejection at the shared static boundary,
  so single-file CLI checks cannot bypass module privacy.
* Explicit exports cannot promote leading-underscore declarations; attempted
  promotion is diagnosed as `E0304` with the export statement span.
* The checker enforces the same private-export rule for single-file static
  checking, not only project diagnostics.
* Static attribute typing now rejects private attributes on module bindings,
  matching interpreter and native module-access checks.
* `set.remove` now reports a missing value instead of silently succeeding,
  matching `list.remove` and native/interpreter behavior.
* Runtime sets now implement zero-argument `clear`, with exact arity and
  frozen-set mutation checks matching the native backend.
* Mutable sets now implement idempotent `discard` in both interpreter and
  native backends; missing values are ignored while frozen sets still reject
  mutation.
* Mutable sets now implement `pop()` in both backends, returning and removing
  one element while reporting an explicit empty-set error. Both backends
  prioritize frozen-set mutation errors consistently.
* Dictionaries now implement `pop(key)` in both backends, returning and
  removing the value with frozen-dictionary and missing-key checks, including
  non-string key representations.
* The syntax crate now exposes a Rowan-backed lossless CST boundary that
  retains source trivia and lexical-error text; the design and migration
  decisions are recorded in [Compiler implementation plan](implementation-plan.md).
* The syntax crate now also exposes a recovering parser API for tooling: it
  keeps successfully parsed statements after a syntax error and returns each
  recovered `ParseError` with its original span, while strict compilation
  remains fail-fast.
* The parser now normalizes hand-built token streams by inserting a terminal
  EOF token, so editor and fuzzing callers cannot trigger an empty-stream
  panic before ordinary parse errors are reported.
* Constant-condition `if` expressions in the typed-HIR primitive subset now
  lower directly to CIR, and the database regression path verifies that this
  bypasses the AST fallback.
* A Salsa-tracked `lower_function_body` query now consumes checked function
  body graphs and lowers primitive expression-only bodies through CIR, with
  explicit errors for missing or unsupported bodies. Unsupported
  multi-statement bodies are rejected rather than silently truncated; bare
  `return` and `pass`, inferred or annotated local sequences, including
  reassignment of a parameter, chained locals, and computed expressions after
  local bindings can now lower through CIR.
  Local rebinding resolves by HIR definition order, so earlier expressions do
  not accidentally observe later assignments.
  Constant statement-level `if` branches with a single selected return are
  folded through the same typed-HIR path, including literal comparisons; a
  statically skipped branch without an `else` lowers to void fall-through.
  Function-level dynamic conditions may now be followed by multiple
  compile-time-resolved `elif` return branches; the first reachable static arm
  is selected without an AST fallback.
  Statically dead `while false` and empty `for` statements before a return are
  also removed when they have no fallback handler; constant `range` bounds and
  steps participate in the same emptiness check.
  Statically true assertions are likewise erased in the primitive linear CIR
  subset; false or dynamic assertions remain outside that subset.
  Constant list, set, and dictionary `for` loops with identifier targets are
  unrolled through the same SSA path; dictionary loops bind keys, matching
  the runtime iteration rule. Wildcard targets are also supported and discard
  each element after lowering it. Integer literal targets filter constant
  iterations according to pattern matching; boolean literal targets are
  supported for collection keys and elements as well. Bounded constant `range`
  loops use the same ordered
  unrolling path, including parameterized function bodies; typed-HIR
  collection defers those loop bodies to the verified unroller so loop-local
  names retain their correct scope. Dynamic iterables and loop-local control
  flow remain outside this linear subset until loop CFG lowering is complete.
  Range bounds and steps may use checked constant integer arithmetic before
  expansion, subject to the existing iteration-size limit.
  Dictionary literal values are now lowered as part of loop setup, so their
  errors cannot be silently skipped while iterating keys.
  The linear lowerer seeds positional parameters as CIR `Param` instructions,
  so a parameterized loop body no longer needs an AST-only fallback.
  Early `return` statements inside an unrolled loop are rejected at this
  boundary rather than being silently converted into later-iteration results;
  they await loop terminators in general CIR lowering.
  A selected single-`pass` branch also lowers to the void ABI.
  Dynamic conditionals with direct returns in both arms now lower to a
  parameter-aware CIR diamond with explicit `Param` reads and a `Phi` merge.
  Void-returning dynamic conditionals retain the same branch structure and
  lower without fabricating a result value.
  Their conditions also support boolean `and`/`or` and identity comparison
  operators in the shared CIR path.
  One-sided dynamic conditionals preserve a value-returning branch and a
  void fall-through branch without synthesizing a result.
  The same mixed ABI handles an explicit `pass` arm.
  A dynamic first condition with a compile-time-known `elif` now folds the
  selected `elif` arm before lowering the remaining dynamic diamond.
  A statically false `elif` without an `else` preserves the dynamic branch's
  void fall-through behavior.
  Dynamic conditional expressions used directly in `return` now lower to the
  same CIR diamond and native result ABI.
  Branch-local assignments followed by `return name` use the same SSA diamond
  and native result ABI, so the local's value is merged explicitly.
  Literal string, float, and `none` equality/identity comparisons are folded
  at the same boundary.
  Integer `+`, `-`, `*`, `//`, and `%` subexpressions in comparisons are
  folded with checked arithmetic before selecting a branch; divide-by-zero and
  overflow remain non-foldable rather than trapping.
  Bitwise `&`, `|`, `^`, and checked shifts are folded as well.
  Unary integer inversion is folded with the same two’s-complement semantics.
  Checked integer exponentiation is folded when the exponent is a
  non-negative machine integer.
  Arbitrary-precision integer literals and promoted machine literals are
  compared without narrowing to `i64`, including unary signs, radix-
  prefixed values, and exponentiation.
  BigInt division, floor-division, and modulo use checked zero-divisor
  handling and Python-style signed remainder rules.
  BigInt bitwise, shift, and inversion operations preserve arbitrary precision
  during constant branch evaluation.
  Native dispatch selection now rejects missing or ambiguous signatures rather
  than silently falling back to the first emitted function, and prefers exact
  matches over `object` fallbacks.
  Native `object` and explicit `Any` parameters now use the tagged `LucidVal`
  representation, allowing universal-value overloads to compile instead of
  emitting undefined C pointer types.
  Calls to universal-value parameters now wrap scalar expressions at the call
  boundary, preserving the C ABI instead of passing raw integers to `LucidVal`.
  Dispatch calls apply the same wrapping after selecting a universal-value
  overload, with native coverage for the `object` fallback itself.
  Union-typed parameters and returns now use the tagged `LucidVal` layout too,
  with execution coverage for an `int | none` return.
  Union-typed scalar parameters are wrapped at call sites, with native
  execution coverage for `int | none` input.
  Native dispatch selection also walks the existing class-parent map and casts
  derived pointers to the selected ancestor signature, matching interpreter
  hierarchy dispatch.
  Missing signature metadata no longer triggers a first-candidate or synthetic
  `Any` fallback; native code generation reports the unresolved dispatch.
  Parameter reads now become explicit CIR `Param` instructions
  and execute through the CIR/native argument seams; async and dispatch
  calling conventions remain rejected until their ABIs exist.
* Cranelift now emits parameterized integer signatures with argument-aware
  invocation coverage for one- through eight-argument CIR functions; larger
  native signatures fail explicitly until a general FFI invocation layer is
  added.
  Recoverable result-ABI calls also accept those parameterized signatures and
  preserve division-by-zero status. The native handle now also exposes a
  parameterized void-call ABI, so CIR functions with bare `return` do not
  enter the integer-return path or trigger an ABI mismatch.
* `emit-cir` and `run-cir` accept `--function NAME`, exposing the typed-HIR
  function-body path through the CLI with integration coverage. `run-cir`
  additionally accepts `--args A,B` for integer parameters and validates the
  arity before entering generated code.
  When the function is representable by the recoverable result ABI, `run-cir`
  now executes that ABI and reports checked division errors instead of falling
  back to the legacy trap-prone integer path.
* Checker union/intersection normalization and callable-parameter metadata
  lookups now use recoverable fallbacks instead of internal panics when fed
  malformed intermediate state.
* Database syntax diagnostics now preserve the parser-reported grammar-error
  span, falling back to the lossless CST lexical span only for lexer failures.
  Recoverable grammar errors are all surfaced instead of truncating diagnostics
  at the first parser failure.
* A new `lucid-db` crate provides the first Salsa-backed compiler database,
  with stable source-file inputs and memoized lossless parsing.
* The database also exposes strict AST parsing and interned top-level symbol
  identities, establishing the input boundary for resolved HIR.
* Type checking is now a Salsa query over `SourceFile`; changing source text
  invalidates and recomputes its semantic result.
* Module-level name resolution is now a Salsa query returning interned symbol
  identities, with source edits invalidating stale bindings.
* Plain `import module` bindings no longer leak the imported module's exports
  into the unqualified namespace; only explicit import bindings are visible,
  while qualified module access remains available.
* Resolved declaration records now capture interned identity, declaration
  kind, and export visibility in an incremental query.
* The database now indexes imports and resolves module paths against an
  explicit project source-file table deterministically.
* Project module ordering is now an incremental dependency-first query that
  rejects cyclic initialization dependencies with a diagnostic.
* Visibility resolution now exposes local declarations and only exported
  declarations from imported modules.
* The CLI file runner now validates source through the Salsa database's strict
  AST and type-check queries before invoking the runtime.
* The CLI exposes `emit-cir`, which prints the deterministic validated CIR for
  a supported initializer after database-backed semantic validation.
* The CLI also exposes `run-cir`, which type-checks, lowers, and executes a
  supported initializer through the owned Cranelift JIT handle.
* `run-cir` now consumes the database's tracked `lower_module` result rather
  than performing a second direct parse/lower path.
* CIR module lowering now folds literal integer, floating-point, and boolean
  comparisons across constant `if`/`elif` chains, including mixed literal
  arms and logical short-circuit forms, without dropping later statements.
* Augmented exponentiation (`**=`) now lowers through the same checked CIR
  power operation as ordinary exponentiation.
* Radix-prefixed integer lexing validates digits as a single literal and
  rejects malformed forms (including stray decimal points) instead of silently
  splitting them into tokens.
* Runtime BigInt literal evaluation now parses the same radix-prefixed forms,
  including values beyond `i64`, so front-end and interpreter representations
  agree.
* Native C emission normalizes radix-prefixed BigInt literals to decimal
  before passing them to its decimal BigInt runtime helpers.
* Constant `not` conditions now use the shared truthiness evaluator, including
  strings and other literal values, while preserving constant `elif` chains.
* Constant conditions bound from comparison results (for example,
  `flag = 1 < 2; if flag`) are now propagated through linear CIR lowering.
  The propagation recursively evaluates checked constant arithmetic operands.
  Boolean `not`, `and`, and `or` instructions are propagated as well.
  Bitwise and checked shift expressions are folded when they feed a condition.
  Checked division, floor division, and remainder are folded as well.
  Arithmetic-bound integer conditions now inherit that same truthiness fold.
  Boolean constants are also accepted as integer-domain comparison operands.
* Dynamic-if `elif` selection now shares full literal truthiness, including
  unary `not` over strings and other literal values.
* Expression-level short-circuit lowering also recognizes literal strings,
  `none`, floats, complex values, sentinels, and ellipsis before lowering the
  right operand.
  Unary `not` is folded recursively on the left side as well.
  Constant comparisons on the left side are folded before any right-hand
  lowering.
  Nested constant `and`/`or` expressions are folded recursively too.
  Literal equality and identity comparisons participate in that folding.
  BigInt truthiness is recognized for arbitrary decimal literals as well.
  Numeric identity comparisons are folded alongside equality comparisons.
  Arbitrary-precision decimal BigInt equality and identity are folded without
  narrowing to `i64`.
* A final dynamic `if` in a linear module now lowers to a verified CFG diamond
  with a Phi merge, preserving prefix bindings and evaluating only the chosen
  branch initializer. Conditions built from already-materialized values may
  use `and` and `or`; standalone initializers still use expression-level
  short-circuit CFG lowering.
* Shape subtyping now treats unknown dimensions directionally: a wildcard
  target accepts any dimension, but an unknown source cannot satisfy an exact
  dimension without evidence.
* Type-level shape rank queries now resolve concrete `typing.shape[...]` ranks
  to integer literals, while unconstrained shapes retain an integer variable.
* Type-level shape products now fold concrete dimensions into a checked
  element-count literal, preserving an integer variable when any dimension is
  unknown and reporting overflow explicitly.
  Overflow behavior has dedicated regression coverage.
* Type-level broadcasting now folds concrete dimensions according to the
  equal-or-one rule and diagnoses incompatible literal dimensions.
* Whole-shape broadcasting now aligns dimensions from the right, retaining
  symbolic unknown axes and producing a concrete `Shape` when possible.
* Type-level matrix multiplication shapes now validate rank and inner-axis
  compatibility, broadcast batch axes, and preserve symbolic dimensions.
* Type-level reshape now compares concrete element counts, rejects provable
  mismatches, and defers validation when dimensions remain symbolic.
* Type-level axis-0 shape concatenation now checks trailing-axis compatibility
  and folds the leading dimension with checked arithmetic.
* Axis-0 concatenation now preserves unknown trailing dimensions instead of
  narrowing them to the known operand's value.
* Type-level shape reversal now produces the exact reversed shape structure.
* Type-level transpose support now swaps the final two shape axes with a
  compile-time rank check.
* Type-level shape insertion now supports checked positive and negative
  indices, preserving exact dimensions for `InsertAt`-style aliases.
* Type-level shape deletion now supports checked positive and negative
  indices, covering `DropAt`-style aliases.
* Shape slicing regression coverage now verifies reverse slices (`S[::-1]`)
  preserve Python-style negative-step traversal and exact dimensions.
  Bounded reverse slices are covered as well, including explicit start and
  stop bounds.
* Shape values now model their documented immutable-list runtime form: they
  satisfy immutable/read-only list views, not mutable `list` parameters.
  The bridge also checks the element type, rejecting immutable `list[str]`
  targets for integer shape values.
* Compile-time shape dimension arithmetic now checks `i64` overflow and emits
  a type diagnostic instead of panicking or wrapping.
* The database now validates complete projects in dependency-first order and
  aggregates per-module semantic diagnostics.
* Typed top-level initializers now carry an interned canonical `TypeId`,
  allowing downstream HIR queries to compare stable identities across files
  while retaining canonical text for diagnostics and snapshots.
* Class, interface, and trait type arguments now honor definition-site
  variance (`+K`, `-K`, and invariant by default) during subtype checks instead
  of comparing only nominal names.
  Definition-site bounds are also validated for every supplied argument, so a
  specialization that violates `T: Bound` is rejected during type resolution.
* Parameterized type aliases retain their formal parameters and substitute
  supplied arguments recursively when the alias is resolved, including arity
  diagnostics for malformed uses. Alias parameter bounds are checked as well,
  and non-parameterized aliases reject unexpected arguments.
* Generic functions retain formal type parameters and infer direct argument
  substitutions for their return type, checking any declared parameter bounds
  at the call site. Nested generic patterns such as `list[T]` are unified
  recursively, and repeated parameters reject conflicting inferred types.
  User-defined function calls also enforce required and maximum arity while
  accounting for defaults and variadic parameters, and validate named
  arguments against declared parameter names without allowing duplicates or
  positional arguments after a named argument; every required parameter must
  be bound, including when the call uses named arguments.
  Parameter defaults are checked against their declared annotations when the
  function is defined. Explicit arguments to non-generic user functions are
  also checked against parameter types at each call site; overloaded dispatch
  names defer that check to overload selection instead of treating the last
  declaration as the only signature, and named arguments are matched to their
  declared positions before type checking. Record literals retain their field
  types, and dictionary literals infer key and value type arguments for
  downstream indexing and calls; statically known record and dictionary
  indexing now returns the element type and diagnoses missing record fields.
  Sequence and shape indexing validates integer indices, and literal shape
  positions preserve their dimension type.
  Record type expressions now resolve to structural record types, including
  duplicate-field diagnostics and structural subtype checks; classes satisfy
  record shapes through inherited fields as well.
  Named attributes on record values, including read-only/immutable views,
  resolve to their declared field types.
  Existential and reified type expressions now retain their underlying
  semantic type, while wildcard and explicit `Never` expressions resolve to
  their corresponding checker types instead of silently collapsing.
  `construct(...)` is restricted to class factories, checks field arity and
  field types, and returns the enclosing class type.
  Assignments to known class fields (including inherited fields and class
  variables) now enforce their declared types.
  Reads of inherited fields use the same recursive lookup and retain the
  declared field type instead of falling back to `Any`.
  Class method and instance method attributes retain callable signatures,
  including inherited methods, so non-direct calls receive argument checks
  and precise return types.
  Getter attributes now expose their declared result types, while setter
  assignments validate the setter parameter type across inheritance.
  Method calls through attributes also validate named arguments against the
  declared method parameter names.
  Retroactive `implement` declarations now publish method signatures, so
  calls to methods added after a class definition receive the same argument
  and return-type checks as native class members.
  Reusable method, getter, and setter bodies supplied by traits also publish
  callable/property signatures to classes that inherit those traits.
  Trait inheritance now composes base obligations and reusable method/property
  signatures recursively, including when a class uses a derived trait.
  Interface declarations now retain method, getter, setter, and inherited
  signatures, allowing implementing-class calls to be checked precisely.
  Calls through values typed as interfaces now use those same signatures,
  including named-argument validation and inherited interface members.
  Trait-typed values now retain reusable method and getter signatures as well,
  including through read-only/immutable views.
  Trait and interface field obligations now retain declared field types, so
  contract-typed field reads no longer fall back to `Any`.
  Setter assignments through trait/interface-typed receivers now validate the
  contract's setter parameter type as well.
  Unknown attributes on closed class, interface, trait, and view types now
  produce static errors instead of silently becoming `Any`, matching Lucid's
  explicit ban on dynamic attribute fallback.
  Unknown attribute writes are rejected as well; only declared fields and
  setters can receive assignments.
  Slice expressions now validate every bound as an integer and reject
  non-sliceable values instead of accepting unchecked slice operands.
  Literal zero slice steps are rejected during checking rather than deferred
  to a runtime failure.
  Indexing now rejects non-indexable values and invalid record index types;
  dynamic valid record indices return the union of possible field types.
  Augmented assignments now type-check their operator operands and resulting
  value against the target, while preserving unknown/protocol values for
  later resolution.
  Calls on known non-callable values now produce static errors; unresolved
  `Any` callables remain deferred for later resolution.
  The CIR linear lowering boundary now handles explicit top-level `return`
  statements, including value-less returns, with verified return terminators.
  Unary plus, negation, and bitwise inversion now validate their operand
  categories instead of accepting arbitrary concrete types.
  Division and exponentiation now reject non-numeric concrete operands rather
  than inferring a numeric result for invalid expressions.
  `await` now requires a `Future` operand, with unresolved `Any` values left
  for later checking.
  The `?` operator now rejects concrete non-recoverable values and unions
  that contain no error variant, while deferring unresolved values.
  Concrete class instances used as `with` contexts must declare `__cm__`,
  while unresolved/function contexts remain deferred.
  Membership operators now validate their right-hand container and known
  element/key types, while allowing user-defined `__contains__` protocols.
  Match exhaustiveness now ignores guarded arms unless an unguarded wildcard
  covers the remaining space.
  `match ... as alias` bindings now enter the checker’s match scope and narrow
  to each arm’s pattern, matching runtime alias behavior.
  Interface and trait subtyping now follows their declared inheritance
  chains, so derived obligations can be used wherever a base contract is
  required.
  Overloaded/dispatch function signatures are retained as a set and calls
  select the unique most-specific compatible candidate. Calls with no
  candidate or multiple incomparable maxima now produce explicit diagnostics
  instead of silently depending on declaration order.
  Repeated ordinary function declarations are rejected unless every overload
  is explicitly marked `dispatch`.
  Generated class constructor calls now enforce inherited field arity, with
  default-valued fields lowering the minimum required argument count.
  Constructor calls also validate named fields, argument ordering, duplicate
  bindings, and concrete field types; bare generic field parameters defer to
  specialization.
  Runtime construction normalization now uses the complete inherited field
  order, so custom subclass factories preserve parent-field values.
  List, set, and dictionary comprehension filters are checked in their
  narrowed iteration scope and must produce `bool`.
  Comprehension iterator, element, key, and value type errors now propagate
  instead of being replaced with `None`.
  List and set literal element errors propagate as well, so undefined names
  cannot be hidden inside collection literals.
  `for` loops reject non-iterables and infer loop bindings for lists, sets,
  dictionaries, ranges, shapes, records, and user classes exposing `__iter__`.
  Immutable `str.chars` views participate in the same iterable recognition and
  yield `str` bindings in loops and comprehensions.
  List, set, and dictionary comprehensions now reject non-iterable sources,
  including raw strings, before inferring their element and key/value types.
  Indexing and slicing immutable/read-only views now reuse the underlying
  sequence or shape element rules instead of being rejected as non-indexable.
  Standalone slice expressions now produce a diagnostic instead of escaping
  the checker as `Any`; slices are valid only as indexing operands.
  Collection mutators invoked through read-only or immutable views now fail
  during checking, before runtime freezing would have to catch them.
  `raise` remains intentionally unchecked as an invariant failure, but its
  exception expression is now type-checked so undefined names cannot escape
  through a raise statement.
  Calls to top-level `contextmanager` functions now have a managed-resource
  type in the checker, so valid `with managed()` code is accepted while
  primitive receivers remain rejected.
  Context-manager checking now requires the single `yield` to be reachable on
  every normal path, rejecting conditionals or loops that can skip it.
  `assert` now accepts either a string message or a zero-argument function
  returning `str`, matching the runtime's lazy-message behavior.
  Positive `is` checks now reject statically disjoint closed types; `is not`
  remains valid for those pairs, while open traits, interfaces, and `Any` stay
  deferred.
  Runtime and checker `is Callable` checks now agree for functions, builtins,
  and partially applied callables.
  Instance-overlap analysis also unwraps mutability views before deciding
  whether a callable contract can apply.
  Native `is Callable` checks now recognize partial-application bindings too,
  so callable identity agrees across both execution backends.
  The native callable builtin registry now includes reflection, freezing,
  file/environment, timing, and trust helpers instead of misclassifying them.
  Calls to the class context-manager hook `__cm__` receive the same managed
  type after member visibility checks.
  Previously permissive builtins now carry explicit checker contracts for
  arity and fixed argument types (`hash`, reflection helpers, iteration
  helpers, `any`/`all`, `zip`, `pow`, and collection constructors).
  Known container-producing builtins (`dict`, `set`, and `sorted`) now retain
  concrete collection result types instead of widening every result to `Any`.
  `list(...)` and `set(...)` preserve the element type of a known collection
  source, including integer shape literals passed through `list`.
  `dict(...)` likewise preserves known key/value types when copying a typed
  dictionary, and string sources infer `list[str]` or `set[str]`.
  `dict(...)` now rejects a statically known non-mapping, non-iterable source
  instead of deferring that mismatch to runtime.
  `list` and `set` constructors now reject statically known non-iterable
  arguments instead of deferring that error to execution.
  Numeric builtins now preserve concrete result types for known inputs:
  `abs`, `round`, and `sum` distinguish integer and floating-point results.
  Their deterministic operand rules are checked as well: `round` requires an
  integer precision, `sum` requires an iterable, and `abs` requires numeric
  input.
  Numeric builtin arity is now explicit for `abs`, `round`, and `sum`, and an
  optional `sum` start value must itself be numeric.
  `pow` now checks numeric base/exponent operands and an integer modulus for
  its three-argument form before backend execution.
  Core conversions and I/O/environment helpers now carry explicit static
  arity contracts matching their runtime entry points, including one-to-three
  argument `range` and the optional `env_var` default.
  `list`, `set`, `help`, and `print` now have explicit zero/one or variadic
  bounds, so extra arguments cannot be silently ignored.
  `range` now checks every supplied bound and step as integers and rejects a
  literal zero step during static checking.
  `complex` now accepts zero to two numeric arguments statically, matching its
  runtime constructor and rejecting non-numeric components.
  The `slice` builtin now enforces one-to-three integer-or-`none` bounds,
  matching its runtime object constructor.
  `ord` now rejects statically known strings other than exactly one Unicode
  character, matching its runtime validation.
  `chr` now rejects statically known integer literals outside Rust's valid
  Unicode scalar range before execution.
  `map` now enforces its two-argument contract, requires a callable mapper,
  and checks its source as an iterable; `enumerate` checks its optional start
  as an integer.
  The documented `fields` reflection builtin is now present in the checker,
  with its one-argument contract and `list[str]` result type.
  Iterable-consuming builtins (`sorted`, `enumerate`, `reversed`, `iter`,
  `any`, `all`, and `zip`) now reject statically known non-iterable inputs;
  `sorted` also matches its runtime one-argument contract.
  `min` and `max` now accept one iterable or multiple values with explicit
  minimum arity, and reject a known non-iterable single argument.
  When their inputs are statically known, `min` and `max` now preserve the
  element type for one iterable or the union of positional argument types.
  `sorted` now applies the same static iterable rule as its runtime
  implementation, which intentionally does not treat raw strings as sortable
  sequences.
  `reversed` now checks the distinct reversible protocol (`__reversed__`) and
  built-in reversible types instead of requiring ordinary `__iter__` alone.
  `len` now requires a known sized type or a declared `__len__` member rather
  than accepting arbitrary values statically.
  Expression statements now run through the checker, so discarded results
  still receive operator, call, and attribute validation.
  The interpreter now rejects `break` and `continue` outside loop bodies as
  runtime errors, matching the checker’s loop-context diagnostics.
  Called functions now receive a fresh loop-control context, so a function
  invoked from a loop cannot accidentally break or continue its caller.
  Runtime dispatch now prefers exact operand types over `object` fallbacks and
  refuses tied best matches instead of selecting the first registration.
  It now walks declared class bases and prefers the nearest matching ancestor,
  so interpreted dispatch follows the same hierarchy model as the checker.
  Top-level `return` statements are rejected by both checker and interpreter;
  unannotated functions still permit returns without imposing a result type.
  Module-level `yield` is likewise rejected outside a context-manager
  definition in both static and direct interpreter execution.
  Built-in list, set, and dictionary methods now have closed-member entries in
  the checker, keeping collection mutation and lookup calls statically visible.
  String methods used by the specification (`split`, `join`, case conversion,
  and trimming) now have concrete callable signatures as well.
  String `chars` now has an immutable list-of-strings type, and `replace`,
  `startswith`, and `endswith` are statically typed with their argument and
  result shapes.
  Class-level `str.bin`, `str.oct`, and `str.hex` factories now have matching
  callable signatures in the checker.
  Class-level numeric constants (`float.inf`, `float.nan`, `int.inf`, and
  `complex.nan`) are now visible to static attribute checking.
  String method and factory arity is checked statically where the runtime
  contract is fixed, including optional `split` and exact-argument methods.
  Concrete string arguments for separators, replacements, prefixes, suffixes,
  and base factories are checked against their runtime-required types.
  Collection method calls now enforce runtime-compatible arity for list, set,
  and dictionary mutation and lookup operations.
  Compile-time shape slicing checks cursor advancement for integer overflow,
  avoiding wrapped dimension positions during type evaluation.
  The checker now publishes signatures for the common collection methods as
  well, so expression statements cannot bypass their callable-member checks.
  Comprehensions now infer yielded element types from user-defined `next()`
  methods instead of collapsing those bindings to `None` or `Any`.
  Set elements and dictionary keys that are statically known mutable
  list/set/dict values are rejected as unhashable.
  Anonymous function parameter and return annotations now propagate type
  resolution errors instead of silently becoming `none`.
  Anonymous closure bodies are checked in their captured environment, with
  declared return types enforced.
  Primitive type names reject supplied type arguments instead of silently
  discarding malformed specializations such as `int[str]`.
  Positional signature validation also rejects a required parameter after a
  defaulted one and rejects duplicate parameter names.
* Duplicate top-level declarations now produce explicit project diagnostics
  instead of collapsing to one symbol index entry.
* Structured per-file diagnostics now carry severity, stable error codes,
  source-file identity, messages, and spans through a Salsa query.
* Lexical parse diagnostics now recover their source range from the lossless
  CST instead of reporting a zero-length placeholder span.
* Project diagnostics now consume those structured file diagnostics, including
  syntax failures that occur before import extraction.
* Resolved declarations retain their source spans, and duplicate-name
  diagnostics now point at the redeclaration location.
* Salsa now exposes a shared byte-offset to line/column source-map query for
  every source file.
* Source-map columns now count Unicode characters rather than UTF-8 bytes,
  keeping diagnostics correctly positioned in non-ASCII source.
* The `lucid check` command now uses the same database-backed validation path,
  keeping command-line diagnostics consistent with execution.
* Native build/run and `eval` entry points also validate through the database
  before invoking their execution or lowering paths.
* CLI commands now discover and validate a sibling `project.yaml` when one is
  present, integrating manifest semantics into the normal workflow.
* A typed `lucid-config` crate now parses the documented `project.yaml`
  metadata, dependency, export, alias, context, and entry-point fields with
  `serde_yaml`.
* The same crate now parses `development.yaml` dependency groups/tool data and
  generated `lucid.lock` package pins with hashes and transitive dependencies.
* Lockfiles now validate duplicate packages, missing dependency references,
  dependency cycles, and malformed/whitespace-bearing content hashes before
  installation or initialization.
* Valid lockfiles now expose deterministic dependency-first install order.
* Project manifests now validate required identity/version semantics,
  duplicate dynamic fields, and dependency names after deserialization.
  Their nested `export` trees now require public identifier keys and public
  internal dotted targets, rejecting malformed YAML shapes at the boundary.
  Entry-point targets enforce the same public-path rule.
  `ProjectConfig::export_target` resolves public dotted names through the
  validated trie for downstream loaders and packagers.
  `ProjectConfig::resolved_exports` flattens the complete public surface into
  a deterministic map for packaging and reflection.
  `ProjectConfig::resolved_entry_points` provides the same deterministic
  packaging boundary for executable commands.
* Manifest file loading now performs semantic validation automatically and
  reports invalid configurations distinctly from I/O and YAML syntax errors.
* Development dependency groups now validate included-group references and
  reject inclusion cycles.
  Group keys and entries also reject empty or whitespace-containing names and
  version requirements before graph expansion.
  Development tool names receive the same validation before tool resolution.
* Development groups can now be expanded into deterministic flattened
  requirements, with conflicting package specifiers rejected.
* The checker now exposes a canonical type boundary that recursively
  normalizes nested function/view/container types and gives unions and
  intersections deterministic, duplicate-free membership.
* Canonical checker types now have a deterministic textual representation
  with sorted class fields, trait/interface lists, and method sets; typed-HIR
  records use it instead of hash-order-dependent debug output.
* Typed-HIR now carries post-order expression graphs for function bodies,
  including defaults, returns, assignments, loops, matches, handlers, and
  context-manager statements, with interned types and source spans.
* Class canonicalization now normalizes trait and interface ordering in the
  semantic `Type` value itself and removes duplicate memberships, so
  equivalent declarations compare equally.
* A new `lucid-cir` crate defines backend-independent blocks, SSA-like
  values, terminators, and a verifier for missing/duplicate references.
* CIR now includes a deterministic reference interpreter for integer
  constants, addition, branches, jumps, and returns, providing a backend
  differential oracle.
* The CIR arithmetic subset now also supports subtraction, multiplication,
  and negation with verifier and execution coverage.
* Integer division is represented in CIR with explicit deterministic
  divide-by-zero errors and lowering coverage.
* Integer remainder is represented and lowered in CIR with the same explicit
  zero-denominator and overflow behavior as division.
* CIR now lowers and executes floor division, bitwise operators, and shifts
  with checked shift/overflow behavior.
* Unary integer inversion (`~`) now lowers and executes through CIR.
* Integer exponentiation now lowers through CIR with checked overflow and an
  explicit error for negative exponents in the integer path.
* CIR now represents and lowers all ordered integer comparisons (`<`, `<=`,
  `>`, `>=`) for shared backend execution.
* Integer inequality (`!=`) now lowers and executes through CIR alongside
  equality.
* Primitive identity operators (`is` and `is not`) now lower through the same
  verified comparison path in CIR and Cranelift.
* Unary logical negation (`not`) now lowers to canonical boolean results in
  the shared CIR reference path.
* Unary plus now lowers through CIR as an integer identity operation.
* Empty and declaration-only modules now lower to verified void CIR with an
  explicit `Return(None)`, allowing the interpreter and native void entry
  points to share the same representation.
* CIR verification now rejects SSA uses that occur before their definition
  within a block, preventing malformed programs from reaching execution.
* The native codegen crate now has a strict Cranelift backend for the verified
  integer CIR subset. It emits executable machine code and supports
  arithmetic, comparisons, bitwise operators, shifts, jumps, and branches,
  supports block parameters for CIR `Phi` merges and canonical boolean logic,
  guards invalid shift counts with explicit traps, and rejects unsupported
  instructions instead of falling back to AST code generation.
* Cranelift native tests now differentially execute representative CIR
  expressions against the CIR reference interpreter, including a regression
  for comparison-width normalization.
* The Cranelift backend exposes a module-level entry point that parses source
  modules through CIR lowering before producing native code.
  It also lowers value-less CIR returns through a distinct void ABI, with a
  guarded `call_void` entry point instead of treating them as integer returns.
  The legacy integer ABI now rejects checked integer division, floor division,
  and remainder before lowering: these operations produce recoverable Lucid
  errors, while the legacy `i64` return cannot carry an error discriminant.
  This explicit validation prevents a machine trap from masquerading as a
  language result; native lowering resumes once the `NativeResult` entry ABI
  is threaded through generated control flow. The shared ABI helper and CIR
  interpreter already define the intended zero-divisor, overflow, and
  negative-divisor semantics.
  A result-returning Cranelift entry point now supports a directly returned
  division, floor-division, or remainder operation with recoverable error
  discriminants; broader result propagation remains to be threaded through
  general CIR control flow.
  Native addition, subtraction, and negation now trap on signed `i64` overflow
  instead of wrapping as machine arithmetic would.
  Native multiplication now performs a checked round-trip overflow test,
  including the `i64::MIN * -1` edge case.
  The legacy native C path now uses checked integer helpers for scalar
  addition, subtraction, and multiplication instead of host-language wrapping.
  An executable native regression confirms `i64::MAX + 1` fails through that
  helper rather than relying only on generated-text inspection.
  Equivalent subprocess coverage now exercises subtraction and multiplication
  overflow paths as well.
  Unary negation of `i64::MIN` now uses a checked native helper with executable
  overflow coverage.
  Native left shifts now verify a checked round trip after shifting, so signed
  overflow traps instead of wrapping.
  The native C path now uses checked shift helpers as well, rejecting invalid
  counts and signed left-shift overflow consistently.
  Negative right shifts use explicit sign extension instead of relying on
  implementation-defined signed-shift behavior.
  Interpreter `for` loops now report range-step overflow instead of wrapping
  the loop cursor, keeping loop execution consistent with native checks.
  List and string slicing also checks cursor advancement for integer overflow
  rather than allowing an extreme step to wrap.
  Native range loops and comprehensions now report the same step-overflow
  failure instead of silently terminating early.
  Constant-exponent Cranelift lowering applies the same checked multiplication
  guards at every power step, preventing exponentiation from wrapping.
* Owned Cranelift handles expose `call_result`, adapting successful native
  integer execution to the shared `NativeResult` contract. Division-family
  instructions are explicitly rejected by the legacy compiler until
  error-capable instructions use that ABI.
  They also expose `call_result_into`, a null-checked caller-owned output
  buffer adapter matching the exported pointer-based ABI.
* A stable `#[repr(C)]` native result ABI now carries an `i64` value and an
  explicit recoverable-error discriminant, ready for error-producing CIR
  operations.
* The ABI exports one canonical mapping from CIR `ExecuteError` values to
  native error discriminants, preventing backend-specific error numbering.
* Native error discriminants implement canonical human-readable formatting so
  diagnostics do not duplicate wording across CLI and backend layers.
* The native ABI now includes a checked integer operation helper covering
  overflow, division, floor division, remainder, and exponent error cases.
* The ABI is extracted into standalone `lucid-abi` and re-exported by codegen
  and runtime, so both consumers share one result layout and error vocabulary.
* That helper is exported as a stable C ABI dispatcher for future Cranelift
  calls; unknown operation tags return `InvalidOperation`.
* `execute_cir` bridges verified CIR execution into the same `NativeResult`
  contract, providing the reference behavior for native result-returning
  entry points.
* CIR verification now computes CFG dominators and rejects values used from a
  block that their definition does not dominate, including branch conditions
  and returns; sibling-branch misuse has regression coverage.
* CIR remainder execution now follows floor-division semantics for both
  positive and negative divisors, with checked quotient/product arithmetic;
  signed-operand cases are covered by the shared oracle tests.
* CIR expression lowering folds constant conditional expressions while
  preserving selective evaluation.
* Dynamic conditional expressions now lower to explicit then/else blocks and
  an edge-based `Phi` merge; CIR verification validates incoming edges and the
  reference interpreter selects only the executed arm.
* CIR Phi validation now requires exactly one incoming per CFG predecessor and
  rejects duplicate or missing edges before execution.
* Phi validation has explicit regression coverage for both duplicate and
  missing predecessor edges.
* CIR logical `and`/`or` lowering now emits short-circuit CFG edges and a Phi
  merge, so dead right-hand operands are never evaluated; canonical boolean
  results remain shared with the reference interpreter.
* Nested logical expressions with constant left operands now preserve the same
  short-circuit behavior during recursive expression lowering, with arithmetic
  composition regression coverage.
* Recursive CIR lowering no longer emits eager logical instructions for
  unsupported dynamic nested cases; those cases fail explicitly until a
  general expression CFG builder handles them.
* Native integer `//` and `%` now use the same floor-division/remainder rule,
  including negative divisors, with a native differential regression test.
* Runtime integer floor division and remainder now report overflow for the
  unrepresentable `i64::MIN / -1` case instead of panicking; both operations
  have regression coverage.
* Runtime floating-point remainder now follows floor-division semantics, and
  native float `//`/`%` lowering uses the same numeric rule with differential
  regression coverage.
* Float modulo now preserves finite dividends with infinite divisors, matching
  IEEE/Python numeric behavior in both runtime and native helpers.
* Runtime modulo now covers mixed `int`/`float` operand pairs with the same
  floor-based result and zero-divisor diagnostics.
* Record destructuring now handles a missing field through a structured
  runtime error instead of an internal `unwrap()` panic.
* Range materialization now uses checked increments and terminates safely at
  `i64` overflow; the shared helper is used by core iterable operations.
  Range-consuming builtins such as `min`, `max`, `sum`, and list/set
  conversion now use that same checked path instead of open-coded increments.
* Native float `//` and `%` now fail on zero divisors like the interpreter,
  instead of silently producing NaN or infinity; a native failure regression
  covers both operators.
* Generated native range and slice loops now use checked stepping and
  overflow-safe cardinality calculations, matching interpreter termination at
  `i64` boundaries.
  Cardinality ceilings use quotient/remainder arithmetic, avoiding unsigned
  overflow for full-width signed ranges.
  Native `for range` loops and range-backed comprehensions use the same checked
  advance helper, including descending ranges.
  Range comprehensions also evaluate the step once and reject zero steps with
  the documented runtime error.
  Native range loops bind all three range arguments once before iteration,
  matching the interpreter’s evaluation order and side-effect behavior.
* Project manifest validation now rejects blank entry-point/library-context
  targets and malformed empty local aliases before execution.
* Manifest validation now rejects whitespace-bearing dependency and alias
  names and requires `library-context` and alias targets to be internal dotted
  paths (while allowing the documented project-root alias `.`).
* Manifest `dynamic` entries now use an explicit metadata allow-list, so
  misspelled or unsupported project fields fail validation instead of being
  silently ignored.
* A field declared dynamic may no longer also be supplied statically, keeping
  manifest metadata's source of truth unambiguous.
* Lockfile validation now applies the same strict identity rules to package
  names, versions, and dependency references, rejecting whitespace-bearing
  graph nodes before ordering or installation.
* Lockfile installation order now uses lexical tie-breaking for independent
  packages and dependencies, so equivalent lockfiles produce identical order.
* Project cycle diagnostics now identify a participating source file and point
  at its first statement instead of returning an unlocated project error.
* Pure integer `and`/`or` expressions now lower through CIR with canonical
  boolean results; effectful operands remain reserved for CFG short-circuit
  lowering.
* CIR execution exposes a configurable instruction-and-terminator step limit
  while retaining the safe one-million-step default; the budget no longer
  depends on how many instructions happen to be grouped into a block.
  CIR also exposes centralized detection of recoverable arithmetic operations
  so front ends can select the result ABI without duplicating the instruction
  set.
  The predicate includes checked overflow, exponent, and shift operations;
  Cranelift keeps its specialized control-flow restriction only for division
  status propagation, where branch-local result merging still needs work.
  Division-family classification is exposed by CIR as well, so that backend
  restriction is shared rather than reimplemented in each caller.
* Module-to-CIR assignment selection now lives in `lucid-cir`; the database
  delegates to that shared lowering API instead of duplicating AST traversal.
* Module-to-CIR lowering now also recognizes initialized `VarDef` statements
  and recursively unwraps `export` wrappers, covering common declaration forms
  without adding a second lowering path.
* The linear CIR lowering boundary now accepts value-return statements and
  rejects source statements after a return until multi-block terminators are
  available.
* CIR functions now have deterministic text serialization for snapshots,
  diagnostics, and future backend differential tests.
* The runtime interpreter now implements `Default` as an embedding-friendly
  alias for its builtin-registering constructor.
* The native code generator now implements `Default`, matching the runtime
  and making backend construction uniform for embedding callers.
* The `check` command now presents the database's structured diagnostics,
  including stable error codes and source line/column locations, instead of
  collapsing them into an unlocated string error.
* Native run validation now uses the same structured database diagnostics,
  including all recoverable parse errors, before loading source modules.
* Interpreted `run` validation now uses that same structured diagnostic path,
  keeping every CLI execution mode consistent.
* The entire Cargo workspace now passes `cargo clippy --workspace
  --all-targets -- -D warnings`; legacy codegen-only style allowances are
  scoped at that crate and do not suppress compiler correctness warnings.
* Configuration now provides validated file loaders for `development.yaml`
  and `lucid.lock`, completing the typed-manifest file boundary.
* CIR now represents boolean constants and equality comparisons, allowing
  source-derived values to drive branch conditions.
* Ordered `<` comparisons are now represented, verified, executed, and
  lowered from AST expressions in the shared CIR subset.
* Unary negation now lowers from AST into the verified CIR arithmetic subset.
* The source-to-CIR bridge has explicit negative coverage for unsupported AST
  forms, preventing accidental fallback semantics.
* CIR verification now separates global SSA identity checks from block order,
  so valid control-flow graphs do not depend on vector layout.
  Phi nodes are now required to form the leading instruction sequence of their
  block, preventing backend-dependent interpretation of malformed SSA.
* CIR arithmetic execution uses checked operations and reports overflow
  explicitly instead of panicking on host `i64` limits, including the
  `i64::MIN / -1` division case.
* CIR function execution now rejects extra positional arguments explicitly;
  missing arguments retain their indexed diagnostic, so interpreter arity
  behavior matches the native ABI.
  Parameterized execution also exposes an explicit step-limit variant for
  deterministic cyclic-control-flow tests and fuzzing.
* CIR now lowers integer literal/add/subtract/multiply AST expressions into
  verified entry-block functions, establishing the first source-to-CIR path.
* Straight-line module lowering now resolves sequential bindings and
  augmented assignments through the same verified SSA boundary.
  Empty and declaration-only modules lower through the database query to the
  shared verified void CIR form.
* Constant-condition `if` statements now select and lower only the reachable
  branch in that boundary; dynamic control flow remains an explicit CIR task.
  Integer conditions use Lucid truthiness (`0` is false, non-zero is true).
* A first dynamic `if` diamond now lowers to branch blocks plus a verified Phi
  merge when both branches assign one literal value; Cranelift executes it
  through the same CFG.
* The same diamond accepts one statically selected `elif` arm, preserving
  branch selection without lowering an incomplete conditional chain.
* The compiler database now exposes an incremental resolved-module HIR
  artifact bundling stable declarations, visibility, spans, and imports.
* Visibility queries now consume that resolved-module artifact directly,
  keeping downstream name lookup off raw AST walks.
* Top-level name resolution now uses the same resolved-module declarations,
  so symbol lookup has one identity-bearing source of truth.
* An incremental `typed_module` query now validates parsed modules and
  depends on the resolved HIR boundary, providing the handoff point for
  expression-level typed HIR and CIR lowering.
* `typed_module` now carries typed top-level initializer records with interned
  symbols, source spans, and canonical checker type data, rather than only a
  boolean validation marker.
* Typed modules now also expose checked top-level function signatures with
  interned parameter and return type identities for downstream lowering.
  Dispatch declarations retain one interned identity for the function name
  plus the complete canonical overload set.
  Parameter names and required/default status are preserved for named-argument
  checking and future ABI lowering.
  Dispatch and async function qualifiers are preserved for backend selection.
  Each typed function also carries a separate post-order expression graph for
  parameter defaults and body statements. Parameter bindings are installed in
  the checker environment while collecting it, so body names retain their
  checked types instead of forcing downstream consumers back to the AST.
  Constant loop bodies are retained as well, with a scoped iteration binding,
  so typed-HIR consumers no longer lose expressions solely because an
  iterable can be folded at compile time.
  Collection now replays validated loop-body statements in that scope, so
  sequential locals introduced inside a loop remain type-resolvable in the
  graph as well.
  The same scoped replay now covers `while` bodies, keeping sequential locals
  visible to typed-HIR consumers there too.
  Loop pattern bindings (including nested tuple, record, class, and starred
  patterns) are seeded in that scope as gradual values for graph collection,
  preventing false undefined-name failures before precise destructuring types
  are lowered.
  Conditional branches now use the same isolated statement replay, preserving
  branch-local bindings and their expression types in the typed graph.
  Match arms now isolate and replay pattern-bound scopes the same way, so
  guards and sequential arm locals retain typed-HIR visibility.
  `try` bodies, handlers, and `finally` blocks now use isolated replay too;
  handler-bound exception names are seeded before collecting their bodies.
  `with` bodies now receive the same scoped replay, including context-target
  pattern bindings and sequential locals.
* CIR now exposes an owned typed-HIR input seam for primitive initializer
  graphs. The database records each initializer's expression root and literal
  payload, converts the checked graph without reparsing source, and prefers
  this lowering path for supported integer/boolean expressions. Unsupported
  typed nodes fall through to the explicit AST lowering boundary until the
  remaining CIR forms are implemented; malformed graph IDs are rejected.
* The database now exposes a tracked `lower_module` source-to-CIR query that
  lowers a verified initializer sequence with binding resolution; the old
  `lower_first_assignment` name is retained as a delegating compatibility
  alias.
* Database CIR lowering now depends on successful typed-module checking, so
  rejected source cannot bypass semantic validation and reach a backend.
* Project-level structured diagnostics now report unresolved imports and
  module-cycle failures with stable error codes, and reject missing or
  non-exported `from ... import` names with `E0302`; import diagnostics now
  point at their source statements.
* The legacy string-facing project checker now formats the structured
  diagnostics query, leaving one authoritative validation path.
* `imported_bindings` resolves `from ... import` names against exported
  declarations and preserves local aliases as stable symbol bindings;
  visible-name lookup now honors those aliases.
* Module visibility now follows the specification: declarations are exported
  within a project by default, while leading-underscore names remain private;
  explicit `export` continues to mark project API declarations.
* Import resolution now handles leading-dot relative module paths based on
  the importing file's package location.
* Package initializer files (`pkg/__init__.lucid`) now resolve as the
  `pkg` module in the incremental source table.
* Relative imports originating inside package initializers now retain the
  package base (`pkg/__init__.lucid` can resolve `.reports` to `pkg.reports`).
* Root-level `__init__.lucid` files now resolve relative imports without a
  spurious leading-dot module path.
* Module initialization order is stable for independent files because the
  source table is sorted by canonical path before graph traversal.
* Project diagnostics reject duplicate module paths with stable `E0303`
  errors instead of allowing source-table overwrites; module ordering rejects
  the same ambiguity at its graph boundary.
* Visible-name lookup now gives local declarations precedence over imported
  aliases, matching lexical shadowing rules.
* `from module import name` visibility is now restricted to the requested
  names; unrelated exports no longer leak into the importing scope.
* Plain `import module [as alias]` now contributes a stable module binding to
  semantic name resolution, matching checker and runtime behavior.
* Duplicate local names introduced by `from` imports are rejected with
  `E0305` instead of being silently deduplicated.
* Duplicate plain-import aliases are rejected with the same `E0305` binding
  diagnostic.
* The checker tracks import-origin bindings and rejects duplicate aliases even
  when invoked without a project database.

The native backend has explicit errors for unsupported lowering cases rather
than silently emitting `none`. That rule is important while the architecture
is being migrated: an accepted program must not acquire accidental semantics
from a missing code-generation branch.

## Code map

The main implementation areas are:

* `compiler/crates/lucid-syntax`: tokens, UTF-8-aware lexing, AST definitions,
  and parsing.
* `compiler/crates/lucid-db`: source-file inputs and incremental syntax queries.
* `compiler/crates/lucid-cir`: validated backend-independent control-flow IR.
* `compiler/crates/lucid-config`: typed `project.yaml`, `development.yaml`,
  and `lucid.lock` loading and validation.
* `compiler/crates/lucid-checker`: name and type checks, capabilities,
  dispatch validation, and static diagnostics.
* `compiler/crates/lucid-runtime`: object values, mutability and freezing,
  results, dispatch, collections, builtins, and async support.
* `compiler/crates/lucid-codegen`: the current interpreter-facing evaluation
  and native C emission paths.
* `compiler/crates/lucid-cli`: command-line entry points and specification test
  discovery.
* `compiler/crates/lucid-cli/tests/spec_tests.rs`: executable checks derived
  from Markdown examples.
* `docs/architecture.md`: the required end-state architecture and completion
  contract.

## Known gaps

The implementation is not yet a complete Lucid compiler. The highest-impact
gaps are architectural rather than isolated syntax features:

1. The lossless CST, source-file inputs, source maps, and structured
   diagnostics exist at the database boundary, but parser recovery and
   source-map propagation through every lowering stage are still incomplete.
   The database now exposes a UTF-8-safe tracked span-to-source lookup for
   diagnostic and editor consumers.
   Source positions also normalize CRLF line endings while retaining
   byte-offset and Unicode-column correctness.
   A tracked inverse `source_offset` query now maps one-based positions back
   to UTF-8 byte offsets, clamping oversized columns to the line end and
   out-of-range lines to end-of-file.
   A tracked `source_line` query provides newline-normalized line text for
   diagnostic and editor rendering.
   Dedicated round-trip coverage now verifies Unicode line/column, byte
   offset, and span extraction agree.
   Malformed reversed spans now safely produce an empty slice rather than
   panicking.
2. Module loading, project manifests, visibility, symbol identity, and
   initialization ordering now have deterministic database/configuration
   foundations; the execution and packaging workflows are not yet fully
   driven by them.
3. The checker now exposes canonical module declarations, typed initializers,
   interned callable signatures (including dispatch overload sets), and a
   post-order expression-level typed-HIR graph for initializer expressions.
   Each node carries an interned canonical type, source span, semantic kind,
   optional operator/name detail, and child IDs. Backends do not yet consume
   this graph universally; function-body and full-language lowering remain
   open, so some name and type information still attaches to ad-hoc AST walks.
4. CIR validation and interpretation now exist for the integer/control-flow
   subset, including SSA dominance and Phi merges, but typed-HIR lowering does
   not yet cover the full language and the native backend still consumes the
   legacy AST path. Differential coverage is therefore incomplete.
5. Generic specialization, representation selection, the complete runtime ABI,
   Cranelift lowering, source-mapped native diagnostics, and foreign/Python ABI
   boundaries remain incomplete.
6. Shape semantics are open in [Shapes](shape.md): literal-count inference for
   dispatch by rank or shape still needs precise rules and tests; type-level
   `stack` shape inference now has a concrete implementation.
7. The typed project/configuration loaders cover core `project.yaml`,
   `development.yaml`, and `lucid.lock` semantics, but installation,
   environment management, library initialization, and full conformance
   coverage remain to be built. Interpreted and zero-argument native
   entry-point execution now covers the basic manifest path.
8. Any syntax or runtime feature not represented by a dedicated checker,
   interpreter, native, and negative test should be considered provisional,
   even if an example currently runs.

## Continuation order

Continue in this order so that new features do not deepen the split between
backends:

1. Build the source-file, module-graph, symbol, visibility, and diagnostic
   layers, including lossless CST spans and deterministic initialization.
2. Replace spelling-based semantic state with canonical interned types,
   resolved HIR, and typed HIR. Make overload selection and coercions explicit.
3. Define and validate CIR, then move the current evaluator behind a CIR
   interpreter. Add differential tests before changing native lowering.
4. Lower CIR through Cranelift with a source map and a single runtime ABI;
   reject every unsupported CIR instruction during validation.
5. Add generic specialization, closure environments, argument-bundle lowering,
   async activation lowering, and representation-aware optimization on that
   common pipeline.
6. Implement project manifests and module boundaries, then close the shape,
   reflection, foreign-ABI, and tooling gaps identified by the specification.
7. Expand the conformance suite so each rule has positive, negative,
   interpreter/native, and module-boundary coverage. Only then assess full
   specification completion.

## Verification loop

For every change, run the focused crate tests first, then the complete checks:

```bash
cargo test --workspace --all-targets --quiet
uv run zensical build --clean --strict
git diff --check
```

When a feature is added, place its executable examples and failure cases in
the relevant specification-test fixture, and verify that both execution paths
agree on output, errors, mutations, and side-effect order. Do not call the
specification complete because the current test count is green; the
architecture and coverage contract above is the completion bar.

The next implementation milestone is the front-end foundation described in
[Implementation architecture](architecture.md), starting with source files,
symbols, and diagnostics. The shape rules in [Shapes](shape.md) should be
resolved alongside that work so later dispatch and lowering decisions have a
stable type model.
