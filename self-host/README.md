# Self-hosting Lucid

Lucid's compiler (`compiler/`) is written in Rust. This directory is the
start of writing pieces of it in Lucid itself — a normal milestone for a
new language's toolchain, not a replacement effort. See the design pillars
in [../docs/principles.md](../docs/principles.md) for the language, and
[../docs/architecture.md](../docs/architecture.md) for the Rust
implementation.

## Status

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

Run it:

```
lucid run self-host/lexer_demo.lucid
```

Self-tokenizing takes on the order of 20-30 seconds under the debug
interpreter build — `lexer.lucid` builds its output strings one character
at a time (`text = text + c`), which is quadratic in a debug build with no
inlining. Worth revisiting once there's more here to justify it (either a
StringBuilder-style pattern in the language, or building lists of
characters and joining once), but it works, which is what "start on"
called for.

The lexer does **not** yet compile under `lucid run --native`: native
codegen has no structural knowledge of the builtin exception hierarchy
(`Exception`, `ParseError`, `ValueError`, ...) the way the checker and
interpreter both do, so `match ... case ParseError:` fails to compile.
That's a real, scoped gap — not specific to this lexer — worth its own
fix later.

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
