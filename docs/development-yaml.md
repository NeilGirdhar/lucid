# `development.yaml`

## Tool configuration

`pyproject.toml` accumulates one `[tool.X]` table per tool a project uses.
Lucid keeps that configuration out of `project.yaml` entirely and collects it
under `tools` in `development.yaml`, keyed by tool name:

```yaml
tools:
  formatter:
    line-length: 88
  linter:
    select:
      - unused-import
      - undefined-name
      - unsorted-imports
      - non-pep585-annotation
    ignore:
      - line-too-long
      - missing-trailing-comma
```
## Development dependencies

`dependency-groups` declares named groups of dependencies that only a
contributor needs: test runners, linters, documentation builders. Unlike
`project.yaml`'s `dependencies` and `optional-dependencies`, these are
never installed for a consumer of the project and are never part of what
`library.initialize` initializes. A group can include another group:

```yaml
dependency-groups:
  test:
    pytest: ">=8.0"
  lint:
    ruff: ">=0.5"
  dev:
    - include-group: test
    - include-group: lint
```
