# Project configuration

Lucid source files use the `.lcd` extension. A Lucid project is configured
by two StrictYAML files at its root: `project.yaml` and
`development.yaml`. Between them, these two files take over the roles
Python spreads across `pyproject.toml`, `setup.cfg`, and the
side-effect code conventionally placed in `__init__.py`.

`project.yaml` describes the project as a dependency: what it needs to run,
what it exposes to other code, and how it is initialized and invoked.
`development.yaml` describes the project as a workspace: the tools used to
build, check, and format it, and the extra dependencies only a contributor
needs. A consumer that only installs and runs a project never needs
`development.yaml`. A third, generated file, `lucid.lock`, pins the
exact dependency versions these two files' specifiers resolved to — see
[lucid.lock](lucid-lock.md).

## Correspondence with `pyproject.toml`

| `pyproject.toml` | Lucid |
| --- | --- |
| `[project] name`, `version`, `description`, `readme` | same, in `project.yaml` |
| `[project] requires-python` | `project.yaml`'s `lucid` |
| `[project] license`, `license-files` | same, in `project.yaml` |
| `[project] authors`, `maintainers`, `keywords`, `classifiers`, `urls` | same, in `project.yaml` |
| `[project] dependencies` | `project.yaml`'s `dependencies` (a name/specifier mapping, also used for library initialization order) |
| `[project] optional-dependencies` | same, in `project.yaml` |
| `[project] dynamic` | same, in `project.yaml` |
| `[project] scripts`, `gui-scripts` | `project.yaml`'s `entry-points` |
| `[build-system]` | not needed; the `lucid` version specifier pins the one toolchain |
| `__init__.py` side effects | `project.yaml`'s `library-context`, run by `library.initialize` |
| `__init__.py` re-exports | `project.yaml`'s `export` and `local-alias` |
| `[dependency-groups]` | `development.yaml`'s `dependency-groups` |
| `[tool.X]` tables | `development.yaml`'s `tools` |
| no native equivalent (`poetry.lock`, `uv.lock`, third-party) | `lucid.lock` |

## Full example

```yaml
# project.yaml
name: acme-inference
version: "2.3.0"
description: Inference serving for Acme models
readme: README.rst
lucid: ">=0.4"
license: Apache-2.0
license-files:
  - LICENSE
authors:
  - name: Ada Lovelace
    email: ada@example.com
urls:
  Homepage: https://example.com/acme-inference
  Repository: https://example.com/acme-inference.git
dependencies:
  numpy: ">=1.26"
  acme-models: ">=1.0,<2.0"
optional-dependencies:
  gpu:
    cupy: ">=13.0"
export:
  models:
    User: .models.User
  parsing:
    parse_user: .parsing.parse_user
local-alias:
  _src: .
  matrix: .some.deep.path.matrix
library-context: .setup.initialize
entry-points:
  serve: .cli.serve
  migrate: .cli.migrate
```
```yaml
# development.yaml
dependency-groups:
  test:
    pytest: ">=8.0"
  lint:
    ruff: ">=0.5"
  dev:
    - include-group: test
    - include-group: lint
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
[project.yaml](project-yaml.md), [development.yaml](development-yaml.md),
and [lucid.lock](lucid-lock.md) cover each file's full structure in turn.
