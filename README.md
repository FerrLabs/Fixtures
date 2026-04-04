# FerrFlow Fixtures

Reusable GitHub Action and CLI tool for generating git fixture repos from declarative TOML definitions. Used by [FerrFlow](https://github.com/FerrFlow-Org/FerrFlow) for integration tests and [Benchmarks](https://github.com/FerrFlow-Org/Benchmarks) for performance testing.

## Usage as GitHub Action

```yaml
- uses: FerrFlow-Org/Fixtures@v0
  with:
    definitions: tests/fixtures/definitions  # path to your TOML definitions
    ferrflow-bin: ./target/release/ferrflow   # path to ferrflow binary
    mode: test                                # "test" or "generate"
```

### Inputs

| Input | Required | Default | Description |
|-------|----------|---------|-------------|
| `definitions` | no | `fixtures/definitions` (bundled) | Path to TOML definitions directory |
| `ferrflow-bin` | no | | Path to ferrflow binary (required for `test` mode) |
| `mode` | no | `test` | `generate` (only build repos) or `test` (build + run ferrflow) |
| `generated-dir` | no | temp dir | Output directory for generated repos |

### Outputs

| Output | Description |
|--------|-------------|
| `generated-path` | Path to the generated fixture repos |
| `passed` | Number of tests passed |
| `failed` | Number of tests failed |

### Example: FerrFlow CI

```yaml
- name: Build ferrflow
  run: cargo build --release

- name: Integration tests
  uses: FerrFlow-Org/Fixtures@v0
  with:
    definitions: tests/fixtures/definitions
    ferrflow-bin: ./target/release/ferrflow
```

### Example: Generate only (for benchmarks)

```yaml
- name: Generate fixture repos
  id: fixtures
  uses: FerrFlow-Org/Fixtures@v0
  with:
    definitions: benchmarks/fixtures
    mode: generate

- name: Run benchmarks against fixtures
  run: ./bench.sh ${{ steps.fixtures.outputs.generated-path }}
```

## Usage as CLI

```bash
# Build the generator
cd generator && cargo build --release

# Generate with defaults (fixtures/definitions/ -> fixtures/generated/)
./generator/target/release/generate-fixtures

# Generate from custom paths
./generator/target/release/generate-fixtures \
  --definitions /path/to/definitions \
  --output /path/to/output

# Run tests (requires ferrflow in PATH)
./scripts/run-tests.sh [generated-dir]
```

## Fixture definition format

Each `.toml` file describes a test scenario:

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
filename = ".ferrflow.toml"  # optional, auto-derived from format if omitted
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
│   └── definitions/           # Bundled example definitions
├── scripts/
│   └── run-tests.sh           # Test runner script
└── .github/
    └── workflows/
        └── test.yml           # CI for the generator itself
```

## License

MIT
