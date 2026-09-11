# Compiler implementation plan

This document chooses the implementation stack for the complete Lucid
compiler. The goal is not to preserve the prototype's internal APIs. The
prototype remains useful as executable reference behavior and regression
fixtures; the new compiler replaces its semantic pipeline behind a clean
boundary.

## Decision in one page

Lucid should use a Rust front end built from Rowan and Salsa, a small
language-specific typed control-flow IR, and Cranelift for native code
generation. The interpreter and native backend consume the same IR. The
runtime remains a Rust library, exposed to generated code through typed ABI
shims. Diagnostics use Ariadne and source positions use `text-size`.

```text
UTF-8 source
  → lexer + Rowan lossless CST
  → recovered AST
  → Salsa queries: modules → symbols → resolved HIR → typed HIR
  → validated Lucid CIR
       ├─ CIR interpreter
       └─ Cranelift lowering → object/linker
```

The old handwritten AST evaluator and C string emitter are not architectural
foundations. They are deleted after their replacement has conformance
coverage.

## Reuse inventory

### Keep and adapt

* The specification Markdown and executable snippets are the language
  contract and become the conformance corpus.
* Existing token spellings, precedence tests, and UTF-8 lexer cases are useful
  behavioral fixtures. Their implementation moves behind the CST lexer.
* The `Value` model, freezing traversal, result propagation, dispatch
  algorithms, and runtime builtins are valuable semantic reference code. They
  move into runtime modules and are called by the CIR interpreter, not AST
  walks.
* Checker tests provide positive and negative examples for variance, views,
  traits, dispatch, and exhaustiveness. They migrate to typed-HIR query tests.
* The CLI's Markdown extraction and temporary native-process harness remain
  test infrastructure; compiler-driver APIs own the behavior.

### Replace

* `ast::Module` as the only source representation: it loses comments,
  recovery information, and stable source identity. Rowan supplies the
  lossless tree; a separate AST/HIR is derived from it.
* Global string-keyed checker state: interned symbols and canonical types are
  required for recursive modules, generic substitution, and incrementality.
* AST-directed execution and C text generation: both duplicate semantics and
  make unsupported cases easy to compile incorrectly. CIR is the only backend
  contract; Cranelift receives validated CIR operations, not source syntax.
* Ad-hoc source spans: use `text_size::TextRange` and a source-file table, with
  line indexing only at diagnostic/rendering boundaries.

## Library choices and rejected alternatives

### Syntax trees and parsing

**Rowan** is the concrete-tree choice. It is lossless, supports cheap
immutable trees and incremental reparsing, and is proven in rust-analyzer.
**Chumsky** is optional for individual grammar productions and recovery
experiments; it must not become a second source-of-truth tree.

Tree-sitter was considered for incremental parsing and editor integrations.
Its external scanner would make indentation, Lucid's context-sensitive
declarations, and precise recovery harder to own in Rust. Rowan with a Lucid
lexer/parser gives tighter control; a tree-sitter grammar can be added later
for editor-only integrations.

### Incrementality and dependency tracking

**Salsa** owns compiler queries and revisions: source text, parse, module
graph, symbol collection, name resolution, type checking, and CIR lowering.
It provides deterministic memoization and dependency invalidation. Hand-rolled
caches are rejected because stale generic or visibility state is a correctness
bug, not merely a performance problem.

### Intermediate representation

Lucid needs a custom CIR because Cranelift IR cannot represent recoverable
results, multiple-dispatch sets, mutability views, cleanup scopes, or source
level pattern coverage without prematurely choosing runtime details. Cranelift
is used only after CIR validation. MLIR was considered, but its binding and
toolchain cost plus broad dialect surface add more integration risk than value
for this language-sized compiler.

### Native backend

**Cranelift** is the default backend: it is Rust-native, supports the target
architectures we need, and avoids untyped C text and dependence on a system C
compiler. LLVM through Inkwell remains an optional later backend for aggressive
optimization or Python-extension packaging. C generation is not a primary
backend.

The initial Cranelift adapter lowers only CIR operations whose ABI is already
defined. It rejects operations that need a structured Lucid error/result ABI;
this is intentional, because emitting target-native wrapping or traps would
silently change recoverable-error semantics. As the CIR runtime ABI grows, the
adapter expands operation-by-operation with interpreter differential tests.

The next ABI milestone is a pair-like native result carrying a value and an
error discriminant (with an optional payload handle). Every CIR instruction
that can produce a recoverable error must return that result and branch through
the same cleanup edges as the interpreter. A machine trap is reserved for
broken compiler/runtime invariants, never for an ordinary Lucid error.

### Diagnostics and tooling

Use `ariadne` for human-readable diagnostics, `text-size` for byte ranges, and
`serde` for machine-readable diagnostic/LSP payloads. Add `tower-lsp` only
after source-file and symbol identities stabilize; an early LSP would freeze
the wrong interfaces.

### FFI and project support

Use `pyo3` for the Python ABI boundary behind an optional Cargo feature. Use
`clap` for CLI parsing and `camino` for UTF-8 project paths. Use `toml` and
`serde` for project configuration and lockfiles. Do not embed a second build
system: Cargo remains the host build while the Lucid manifest controls module
discovery and Lucid-specific options.

### Correctness tooling

Use `insta` for parser/HIR/diagnostic snapshots, `proptest` for algebraic
properties (hash/equality, dispatch specificity, and shape operations), and
`cargo-fuzz` for lexer/parser recovery. Every CIR program runs through both
the interpreter and Cranelift backend and compares output, errors, mutations,
and cleanup order.

## Replacement sequence

1. **Front-end boundary:** finalize Rowan token kinds, trivia, error nodes,
   file IDs, and source maps. Make parsing incomplete input total and keep the
   strict parser only as a temporary AST adapter.
2. **Compiler database:** add Salsa inputs/queries and deterministic module
   loading. Introduce interned `FileId`, `SymbolId`, `TypeId`, and
   `DispatchSetId`; no new semantic code keys state by printed names.
3. **HIR and checker:** lower CST to resolved HIR, then typed HIR with
   explicit coercions, selected overloads, mutability views, and exhaustiveness
   facts. Port checker tests before deleting old checker walks.
4. **CIR:** define instruction and terminator enums, verifier, cleanup edges,
   source spans, and a serializer used by snapshots. Lower every supported
   construct and reject missing cases before execution.
5. **Interpreter:** move runtime semantics behind CIR operations. Differential
   tests become the gate for deleting AST evaluation paths.
6. **Cranelift:** lower each verified CIR instruction using one ABI for values,
   objects, results, closures, and async activations. Remove C emission after
   native differential coverage reaches parity.
7. **Generics, ABI, and tooling:** add specialization/representation policy,
   Python integration, manifests/lockfiles, LSP queries, and optimizations
   only after semantics are shared.

## Non-negotiable gates

No phase is complete merely because existing tests pass. A phase must provide

* positive and negative tests with source spans;
* interpreter/native differential tests;
* module-boundary tests where visibility or initialization is involved;
* a failure for every deliberately unsupported operation; and
* strict documentation build and reproducible command-line output.

The current prototype remains useful until each gate is met, but it is not
part of the final architecture. This plan is the basis for the migration
tracked in [Implementation architecture](architecture.md) and the status in
[Implementation handoff](handoff.md).
