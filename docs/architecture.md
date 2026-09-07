# Compiler and runtime architecture

Lucid separates language validation from code execution. The toolchain
processes a program through three distinct stages: syntactic analysis,
static verification, and execution.

Execution offers two complementary backends: an ahead-of-time native compiler
that generates optimized machine code through C99, and a tree-walking
reference interpreter for rapid development and interactive evaluation.

The system is structured as a Cargo workspace in `compiler/crates/`:

* `lucid-syntax`: Lexer, token stream, and recursive-descent parser.
* `lucid-checker`: Static semantic analysis, type checking, and invariant
  validation.
* `lucid-codegen`: Native code generator emitting C99 machine code via GCC.
* `lucid-runtime`: Tree-walking evaluator, value representations, and
  built-in functions.
* `lucid-cli`: Unified command-line driver uniting compilation, execution,
  type checking, and the interactive read-eval-print loop.

## Pipeline stages

Every source file moves through a linear front-end pipeline before entering an
execution backend:

1. *Source text*: A UTF-8 text file containing Lucid statements and expressions.
2. *Token stream*: The lexer converts source text into tokens, translating
   indentation changes into synthetic indentation tokens.
3. *Abstract syntax tree*: The parser groups tokens into typed syntax tree nodes
   representing modules, declarations, statements, and expressions.
4. *Semantic analysis*: The type checker verifies contracts, validates single
   inheritance, checks pattern exhaustiveness, and confirms mutability
   permissions.
5. *Code generation or interpretation*: The program either compiles to a
   native ELF binary through C99 or evaluates directly in the reference
   runtime.

## Front-end: syntax and parser

The `lucid-syntax` crate converts source text into an abstract syntax tree.

### Lexical analysis

The lexer tracks source line and column numbers while resolving whitespace
indentation depths. It maintains an internal stack of active indentation
levels:

* When a line begins at a greater indentation depth than the stack top, the
  lexer emits an *indent token* and pushes the new depth.
* When a line begins at a lesser indentation depth, the lexer pops depths until
  matching the active level, emitting a *dedent token* for each popped depth.
* When indentation does not match any enclosing level on the stack, the lexer
  reports an indentation error.

This indentation mechanism removes the need for braces while retaining unambiguous
block structure.

### Syntactic parsing

The parser uses recursive descent with precedence climbing for binary
expressions. It produces an abstract syntax tree rooted in the `Module` node.

The syntax tree preserves language semantics:

* Separate declaration nodes for `interface`, `trait`, and `class` types,
  enforcing the separation between obligations, reusable behavior, and owned
  state.
* Explicit mutability annotations on type references: mutable `T`, read-only
  view `&T`, and deeply immutable object `!T`.
* Multiple dispatch annotations on operator definitions: `dispatch def`.
* Error propagation syntax: the postfix `?` operator on expressions.

## Static checker: semantic analysis

The `lucid-checker` crate validates programs before evaluation or code
generation begins.

### Type checking and inference

Lucid uses bidirectional type checking. Function signatures require explicit
parameter and return types. Within function bodies, local variable types infer
from initial assignments unless explicitly annotated.

The checker rejects mismatched assignments, invalid function arguments, and
undefined attribute accesses at compile time.

### Contract and inheritance validation

The checker enforces structural rules defined in the specification:

* *Interface satisfaction*: An interface defines obligations without method
  bodies or fields. The checker verifies that any concrete type claiming to
  implement an interface provides matching method signatures.
* *Trait composition*: A trait provides reusable method bodies without fields.
  The checker ensures traits compose without colliding implementations.
* *Single class inheritance*: A class contains owned fields and constructors.
  The checker rejects any class declaration declaring more than one parent
  class.

### Pattern exhaustiveness

The `match` statement requires complete pattern coverage over variant types
and algebraic shapes. The checker analyzes all match branches against the
target type. If an unhandled case exists, compilation halts with an error
pointing to the missing pattern.

### Mutability verification

Lucid tracks mutability permissions through three view types:

* *Mutable reference*: Written as `T`; permits field mutation.
* *Read-only view*: Written as `&T`; forbids mutating attributes through
  this reference while permitting reads.
* *Deeply immutable object*: Written as `!T`; guarantees the instance and all
  transitively reachable state cannot mutate.

The checker rejects attribute mutation statements whenever the target
expression evaluates to a read-only view or a deeply immutable object.

## Native code generation

The `lucid-codegen` crate translates the verified abstract syntax tree into
optimized machine code by targeting C99.

### Execution model

Dynamic languages often box numbers in heap wrappers and execute bytecode
through virtual dispatch loops. Lucid compiles directly to native instructions
using GCC or Clang:

```bash
lucid build program.lucid -o program
```

The code generator emits a standalone C99 source file and invokes `gcc -O3`
with link-time optimizations to produce an ELF executable.

### Unboxed hardware representations

Primitive types map directly to machine registers:

* The `Int` type maps to `int64_t`.
* The `Float` type maps to `double`.
* The `Bool` type maps to `bool`.

Arithmetic operations compile to native CPU instructions (such as `addq`,
`imulq`, and `sqrtsd`) without heap allocations, type tag inspections, or
reference counting checks.

### Contiguous memory layout

Fixed-size arrays allocate contiguous flat memory buffers:

```c
double* a = (double*)malloc(n * sizeof(double));
```

Indexed loops access memory through stride pointer offsets. This layout avoids
pointer chasing, keeps data inside CPU cache lines, and lets the C compiler
vectorize inner loops with SIMD instructions.

### Hardware call stacks

Function invocations translate directly to machine calls:

```c
int64_t fib(int64_t n) {
    if (n <= 1) return n;
    return fib(n - 1) + fib(n - 2);
}
```

Because calls use native CPU stack frames rather than heap-allocated virtual
frames, recursive algorithms and tight loops incur zero interpreter dispatch
overhead.

## Reference interpreter

The `lucid-runtime` crate provides an alternative tree-walking execution
engine.

### Value representation

The interpreter represents runtime values using a tagged enum:

* Primitive values: integers, floating-point numbers, booleans, and strings.
* Collection values: lists, dictionaries, sets, and anonymous records.
* Callable values: user functions, closures, and built-in primitives.
* Object instances: class instances storing field tables and parent pointers.

### Dynamic environment

The interpreter maintains lexical environment frames. When entering a scope,
the runtime allocates an environment record holding local variables and a
pointer to the parent scope.

The reference interpreter executes source files immediately without requiring
a C compiler:

```bash
lucid run program.lucid
```

### Interactive REPL

The interpreter powers the interactive read-eval-print loop:

```bash
lucid repl
```

The REPL persists definitions across input lines, allowing interactive
experimentation with algorithms, traits, and types.

## Developer tooling and commands

The `lucid-cli` crate provides the command-line driver for the language:

* `lucid check <file>`: Runs the lexer, parser, and static type checker,
  reporting compile errors without executing code.
* `lucid build <file> -o <bin>`: Emits C99 code, compiles with GCC at `-O3`,
  and outputs a standalone native executable.
* `lucid run --native <file>`: Compiles and runs the file natively in a
  single step.
* `lucid run <file>`: Runs the file through the reference tree-walking
  interpreter.
* `lucid emit-c <file>`: Prints the generated C99 code to standard output
  for inspection and optimization audits.
* `lucid eval "<code>"`: Evaluates a short code string directly.
* `lucid test-spec [dir]`: Extracts code examples from specification
  documents and verifies that each example parses and passes type checking.

## Performance characteristics

The Computer Language Benchmarks Game suite evaluates compiler performance
against CPython 3.14 across ten standard compute benchmarks.

Across all ten benchmarks, the native AOT compiler achieves a 13.7x geometric
mean speedup over CPython 3.14:

* **Spectral norm**: 40.3x faster than CPython 3.14 due to flat array layout
  and inner loop vectorization.
* **Fibonacci recursion**: 36.5x faster than CPython 3.14 due to native
  hardware stack frames.
* **Mandelbrot**: 18.2x faster than CPython 3.14 through unboxed 64-bit floating
  point operations.

Detailed measurements, benchmark implementations, and replication scripts
reside in the benchmark suite documentation in `BENCHMARKS.md`.

For language design rationale and type system semantics, consult
[Main ideas](principles.md) and
[Modern type specification](type-specification.md).
