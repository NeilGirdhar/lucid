# Getting started with Lucid

Lucid is a statically-typed, expressive language design combining Python's syntax elegance with compile-time type safety, Julia-style multiple dispatch, explicit mutability views, and ergonomic result-based error handling.

This guide walks through building the Lucid compiler, using the interactive REPL, and executing scripts. See [Language tour](language-tour.md) for a walkthrough of the language features themselves.

## Installation and setup

### Prerequisites

- **Rust toolchain** (edition 2024 compatible, Rust 1.85+ recommended).

### Building from source

Clone the repository and build the workspace:

```bash
git clone https://github.com/npow/lucid.git
cd lucid

# Build release binary
cargo build --release

# The compiled binary is located at:
# ./target/release/lucid
```

Optionally, add `./target/release` to your `PATH`, or alias it:

```bash
alias lucid="$(pwd)/target/release/lucid"
```

## Using the Lucid CLI

The `lucid` command-line tool provides everything needed to run, typecheck, evaluate, and experiment with Lucid code.

```
Lucid Language Compiler & Runtime

USAGE:
    lucid [COMMAND] [OPTIONS]

COMMANDS:
    repl                 Start interactive REPL (default when no arguments)
    build <file> [-o bin] Compile source file to an optimized native binary
    run <file> [--native] Run a Lucid source file (interpreted or natively compiled)
    check <file>         Parse and typecheck a Lucid source file
    emit-c <file>        Emit generated C99 code for a Lucid source file
    eval <code>          Evaluate a Lucid code snippet string
    test-spec [dir]      Extract and validate code snippets from RST specification docs
    help                 Display this help message
    version              Show version information
```

### Compiling to native executables

Lucid includes an ahead-of-time (AOT) native compiler that translates Lucid code
to optimized machine code via C99 and GCC (`-O3`), with unboxed 64-bit hardware
types and flat contiguous memory buffers:

```bash
# Compile to a standalone binary
lucid build examples/hello.lucid -o hello
./hello

# Or compile and run directly in one step
lucid run --native examples/hello.lucid
```

For technical details on how the compiler and runtime function, see
[Compiler and Runtime Architecture](architecture.md) and [Benchmark Performance](benchmarks.md).

### Interactive REPL

Launch the REPL simply by running `lucid` or `cargo run -p lucid-cli`:

```bash
$ lucid
Lucid 0.1.0 interactive REPL
Type :help for assistance, :exit or :quit to leave.

>>> 2 + 2
4
>>> words = "lucid is clean and expressive".split()
>>> [w.upper() for w in words if len(w) > 4]
["CLEAN", "EXPRESSIVE"]
>>> def add(a: int, b: int) -> int:
...     return a + b
... 
>>> add(10, 32)
42
>>> :exit
```

**REPL commands:**
- `:help` — Display REPL tips and available commands.
- `:vars` — List currently defined variables and values in the environment.
- `:clear` — Clear the screen.
- `:reset` — Reset the environment and type checker.
- `:exit` (or `:quit`) — Exit the REPL.

### Running files (interpreted)

Execute a Lucid source file (`.lucid`) using the reference tree-walking interpreter:

```bash
lucid run examples/hello.lucid
```

### Typechecking files

Verify types without executing runtime code:

```bash
lucid check examples/hello.lucid
```

### Evaluating one-liners

```bash
lucid eval "print([x * x for x in range(6)])"
# Output: [0, 1, 4, 9, 16, 25]
```

## Editor support (VS Code)

Syntax highlighting and language configurations are included in `editors/vscode`:

```bash
# Link the extension into VS Code
ln -s "$(pwd)/editors/vscode" ~/.vscode/extensions/lucid
```

Then reload VS Code. All `.lucid` files will have full syntax coloring, auto-closing brackets, and intelligent indentation.

## Running specification tests

Verify the entire language specification test suite:

```bash
# Run all unit and integration tests
cargo test --workspace

# Validate documentation code snippets against the parser and checker
cargo run -p lucid-cli -- test-spec
```
