# FerrFlow Fixtures

Reusable GitHub Action and CLI tool for generating git fixture repos from declarative TOML definitions. Used by [FerrFlow](https://github.com/FerrFlow-Org/FerrFlow) for integration tests and [Benchmarks](https://github.com/FerrFlow-Org/Benchmarks) for performance testing.

Fixtures is a pure generator — it builds repos from TOML definitions but does not run any tests. Each consumer repo (FerrFlow, Benchmarks, etc.) owns its own definitions and test runner.

## Usage as GitHub Action

```yaml
- name: Generate fixture repos
  id: fixtures
  uses: FerrFlow-Org/Fixtures@v0
  with:
    definitions: tests/fixtures/definitions

- name: Run tests against fixtures
  run: ./scripts/run-fixture-tests.sh ${{ steps.fixtures.outputs.generated-path }}
```

### Inputs

| Input | Required | Default | Description |
|-------|----------|---------|-------------|
| `definitions` | **yes** | | Path to TOML definitions directory (provided by the consumer repo) |
| `generated-dir` | no | temp dir | Output directory for generated repos |

### Outputs

| Output | Description |
|--------|-------------|
| `generated-path` | Path to the generated fixture repos |

## Usage as CLI

```bash
# Build the generator
cd generator && cargo build --release

# Generate from custom paths
./generator/target/release/generate-fixtures \
  --definitions /path/to/definitions \
  --output /path/to/output
```

## Fixture definition format

Each `.toml` file describes a git repo scenario:

```toml
[meta]
name = "monorepo-two-packages"
description = "Two packages with independent version bumps"

[config]
content = '''
{
  "package": [
    { "name": "core", "path": "core", "versioned_files": [{"path": "core/version.toml", "format": "toml"}] },
    { "name": "cli", "path": "cli", "versioned_files": [{"path": "cli/version.toml", "format": "toml"}] }
  ]
}
'''

[[packages]]
name = "core"
path = "core"
initial_version = "0.1.0"

[[packages]]
name = "cli"
path = "cli"
initial_version = "0.1.0"

[[commits]]
message = "feat(core): add parser"
files = ["core/src/parser.rs"]

[[commits]]
message = "fix(cli): handle empty input"
files = ["cli/src/main.rs"]

[expect]
check_contains = ["core", "0.2.0", "cli", "0.1.1"]
check_not_contains = ["Nothing to release"]
packages_released = 2
```

The `[expect]` section is ignored by the generator — it's metadata for the consumer's test runner.

### Bulk generation

For benchmarks or stress tests, use `[generate]` to create repos with many packages and commits without listing them individually:

```toml
[meta]
name = "mono-large"
description = "200 packages, 10000 commits"

[config]
content = '{}'

[generate]
packages = 200    # number of packages (1 = single-package repo)
commits = 10000   # number of synthetic commits
seed = 42         # optional RNG seed for deterministic output
```

Uses an incremental tree builder for fast generation (10k commits in under a minute).

### Advanced features

#### Tags at arbitrary commits

```toml
[[tags]]
name = "v1.0.0"
at_commit = -1  # -1 = initial setup commit, 0+ = index into [[commits]]

[[tags]]
name = "v1.1.0"
at_commit = 2  # after the third commit
```

The old-style `tag` field on `[[packages]]` still works for tags on the initial commit.

#### Config format selection

```toml
[config]
format = "toml"             # "json" (default), "toml", "json5"
filename = "ferrflow.toml"   # optional, auto-derived from format if omitted
content = '''
...
'''
```

#### Hook scripts

```toml
[[hooks]]
path = "hooks/pre-bump.sh"
content = '''#!/usr/bin/env bash
echo "running pre-bump"
'''
```

#### Custom default branch

```toml
[meta]
name = "my-fixture"
description = "Repo with master as default branch"
default_branch = "master"  # defaults to git's init.defaultBranch if omitted
```

#### Multiple branches

Create branches from specific points and optionally merge them back:

```toml
[[branches]]
name = "develop"
from = "main"           # source branch (defaults to default branch)
at_commit = 0           # branch from this commit index (-1 = initial, 0+ = commit index)
merge = "main"          # optional: merge back into this branch when done
commits = [
  { message = "feat: new feature on develop", files = ["src/develop.rs"] },
  { message = "fix: develop bugfix", files = ["src/develop-fix.rs"] },
]

[[branches]]
name = "feature/analytics"
from = "main"           # branches from tip of main if at_commit is omitted
commits = [
  { message = "feat: add analytics", files = ["src/analytics.rs"] },
]
```

- `from`: source branch name (defaults to `default_branch`)
- `at_commit`: commit index to branch from (`-1` = initial setup, `0+` = index into `[[commits]]`). If omitted, branches from the tip of `from`.
- `merge`: if set, creates a merge commit back into the named branch after all commits are added
- `commits`: list of commits to add on this branch (same format as top-level `[[commits]]`)

#### Merge commits

```toml
[[commits]]
message = "feat: merged feature"
files = ["src/feature.rs"]
merge = true
```

## Directory structure

```
.
├── action.yml                 # GitHub Action definition
├── generator/                 # Rust binary that builds fixture repos
│   ├── Cargo.toml
│   └── src/main.rs
├── fixtures/
│   └── examples/              # Example definitions for reference
└── .github/
    └── workflows/
        └── test.yml           # CI for the generator itself
```

## License

MIT
