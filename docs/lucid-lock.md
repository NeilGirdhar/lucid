# `lucid.lock`

`project.yaml`'s `dependencies` and `development.yaml`'s
`dependency-groups` declare intent: version specifiers, not exact
versions. Two installs run days apart, or on two different machines, can
resolve those specifiers to different actual versions unless something
pins the result down. `lucid.lock` is that pin: the toolchain writes
it, recording the exact version and a content hash for every dependency
the project actually resolved to, direct and transitive alike.

```yaml
# lucid.lock
package:
  - name: numpy
    version: "1.26.4"
    hash: "sha256:4c66..."
    dependencies: []
  - name: acme-models
    version: "1.4.2"
    hash: "sha256:9be0..."
    dependencies:
      - numpy
```
Resolution happens once, not on every install: the toolchain re-resolves
only when `lucid.lock` is missing, when `project.yaml` or
`development.yaml` change what they require, or when a contributor
explicitly asks for an update. Everyone else — a teammate cloning the
project, a CI run, a production deploy — installs exactly what
`lucid.lock` already says, the same version and the same bytes every
time, not whatever the specifiers happen to resolve to today.

`lucid.lock` is generated, never hand-written, the same way
`project.yaml`'s `dynamic` fields are computed rather than typed in.
It is committed for a project with [Entry points](project-yaml.md#entry-points) — an application,
run the same way on every machine it is deployed to — but not
necessarily for a project that exists only to be depended on: a
library's own consumers resolve its specifiers against their own lock
file, and shipping one pinned resolution would just override that.
