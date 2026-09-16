# CLI tooling roster (proposal)

Pending discussion with Nissan — not decided, and nothing here beyond
what's already implemented today should be treated as settled.

A sketch of the `lucid` CLI's tool surface, one verb per job, rather
than the current implementation's ad hoc naming (`build`, `check`):

- **compile** — turn source into an executable. Today: `lucid build
  <file> -o <bin>`.
- **run** — run source directly. Today: `lucid run <file>`
  (interpreted) / `lucid run --native <file>` (compiled).
- **lint** — linter and type checker combined. Today: `lucid check
  <file>` already does this — it parses, type-checks, and reports
  checker warnings such as the unmarked-variance one.
- **format** — reformat source in place. Not implemented.
  `development.yaml` already names a `formatter` tool key
  (`development-yaml.md`) with no built-in behind it.
- **test** — run a project's own tests. Not implemented.
  `development.yaml` already names a `test` dependency group
  (`development-yaml.md`) with no built-in runner. This is distinct
  from `lucid test-spec`, which tests the *compiler* against the
  specification's own examples, not a user's project.
- **doc** — generate documentation from a project's `fields()`-visible
  docstrings and metadata (`class-members.md`'s field docstrings and
  metadata section). Not implemented.

## Open questions

- Do `compile`/`lint` replace `build`/`check` outright, or coexist as
  aliases?
- Does `format` get a `--check` mode, the way this repo's own `uv run
  zensical build --clean --strict` is used in CI rather than writing
  changes?
- Is `doc`'s output format decided — a built HTML site the way this
  spec itself is built with Zensical, or something else?
