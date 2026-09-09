# The Lucid language

Lucid is a language for LLMs: a Python-like language sketch designed to be as
easy for an LLM to read and write correctly as for a human.

Core principle:

> Keep Python's directness, make structure explicit, and choose the cleaner
> rule when compatibility no longer has to win.

The full specification lives in [`docs/`](docs/index.md), or rendered at
**<https://neilgirdhar.github.io/lucid/>** — start at
[`docs/index.md`](docs/index.md) for the worked example; its sidebar links
to the rest of the specification.

## Implementation and tooling

Lucid includes a compiler and toolchain written in Rust, located in
`compiler/crates/`:

* `lucid-syntax` — lexer, token definitions, and recursive-descent parser.
* `lucid-checker` — static checker verifying types, mutability views (`T`,
  `~T`, `!T`), trait bounds, and exhaustive matching.
* `lucid-codegen` — ahead-of-time (AOT) native code generator producing
  optimized machine code via C99/GCC with unboxed numeric registers and hardware
  call stacks.
* `lucid-runtime` — reference evaluation engine and dynamic dispatch runtime.
* `lucid-cli` — the unified driver executable providing `build`, `run`,
  `check`, and `parse` subcommands.

### Quick start

Build the release toolchain with Cargo:

```bash
cargo build --release
```

Compile a Lucid source file to a native binary:

```bash
./target/release/lucid build program.lucid -o program
./program
```

Run a file natively with immediate execution:

```bash
./target/release/lucid run --native program.lucid
```

Type-check a file and report diagnostics without compiling:

```bash
./target/release/lucid check program.lucid
```

### Benchmarks

Lucid includes an automated benchmark suite adapted from the Computer Language
Benchmarks Game (Debian/Ubuntu Shootout). The suite evaluates 10 problems
against CPython 3.14, measuring recursive call overhead, memory indexing, dense
matrix products, floating-point iteration, and object trees.

To run the suite:

```bash
python3 benchmarks/run_benchmarks.py
```

Lucid Native achieves a 13.7x geometric mean speedup over CPython 3.14 across
all benchmarks with exact numerical output equivalence. Detailed measurements
and architectural notes are in [docs/benchmarks.md](docs/benchmarks.md).

