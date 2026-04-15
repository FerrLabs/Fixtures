# FerrFlow Fixtures

Reusable GitHub Action and CLI tool for generating git fixture repos from declarative JSON definitions. Used by [FerrFlow](https://github.com/FerrFlow-Org/FerrFlow) for integration tests and [Benchmarks](https://github.com/FerrFlow-Org/Benchmarks) for performance testing.

Fixtures is a pure generator — it builds repos from JSON definitions but does not run any tests. Each consumer repo (FerrFlow, Benchmarks, etc.) owns its own definitions and test runner.

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
| `definitions` | **yes** | | Path to JSON definitions directory (provided by the consumer repo) |
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

Each `.json` file describes a git repo scenario. Add `$schema` for editor autocomplete and validation:

```json
{
  "$schema": "https://raw.githubusercontent.com/FerrFlow-Org/Fixtures/main/schema/fixture.schema.json",
  "meta": {
    "name": "monorepo-two-packages",
    "description": "Two packages both touched in the same commit get independent bumps"
  },
  "config": {
    "content": "{ \"package\": [ { \"name\": \"core\", \"path\": \"core\", \"versioned_files\": [{\"path\": \"core/version.toml\", \"format\": \"toml\"}] }, { \"name\": \"cli\", \"path\": \"cli\", \"versioned_files\": [{\"path\": \"cli/version.toml\", \"format\": \"toml\"}] } ] }"
  },
  "packages": [
    { "name": "core", "path": "core", "initial_version": "0.1.0", "tag": "core@v0.1.0" },
    { "name": "cli", "path": "cli", "initial_version": "0.1.0", "tag": "cli@v0.1.0" }
  ],
  "commits": [
    { "message": "feat(core): add parser", "files": ["core/src/parser.rs"] },
    { "message": "fix(cli): handle empty input", "files": ["cli/src/main.rs"] }
  ],
  "expect": {
    "check_contains": ["core", "0.2.0", "cli", "0.1.1"],
    "check_not_contains": ["Nothing to release"],
    "packages_released": 2
  }
}
```

The generator copies the `[expect]` section into a `.expect.toml` file at the root of each generated repo. Consumer repos (FerrFlow, Benchmarks) use this file to validate their test runners against expected output.

### `.expect.toml` contract

The generated `.expect.toml` contains the fixture's `description` (from `[meta]`) plus the fields from `[expect]`. All fields except `description` are optional.

| Field | Type | Description |
|-------|------|-------------|
| `description` | string | Human-readable description of the fixture scenario. Copied from `meta.description`. |
| `check_contains` | string[] | Strings that **must** appear in the consumer's output (case-sensitive, fixed-string match). |
| `check_not_contains` | string[] | Strings that **must not** appear in the consumer's output. |
| `output_order` | string[] | Strings that must appear in order. Consumer runners should also verify blank-line separation between consecutive items. |
| `packages_released` | integer | Expected number of packages released. |

Example generated file:

```toml
description = "Two packages with independent version bumps"

check_contains = [
    "core",
    "0.2.0",
    "cli",
    "0.1.1",
]

check_not_contains = [
    "Nothing to release",
]

packages_released = 2
```

**Consumer responsibilities:**

- Run the tool under test (e.g. `ferrflow check`) inside the fixture directory
- Strip ANSI escape codes from output before matching
- Validate each field present in `.expect.toml`:
  - `check_contains`: every string must appear in the output. Empty array = skip this check.
  - `check_not_contains`: no string may appear in the output. Empty array = skip this check.
  - `output_order`: strings must appear left-to-right by byte offset, with blank lines between consecutive items. Empty array = skip.
  - `packages_released`: count of packages with version bumps must match. If absent, skip this check.

See [FerrFlow's `run-tests.sh`](https://github.com/FerrFlow-Org/FerrFlow/blob/main/tests/fixtures/run-tests.sh) for a reference implementation.

### Bulk generation

For benchmarks or stress tests, use `generate` to create repos with many packages and commits without listing them individually:

```json
{
  "meta": {
    "name": "mono-large",
    "description": "200 packages, 10000 commits"
  },
  "config": { "content": "{}" },
  "generate": {
    "packages": 200,
    "commits": 10000,
    "seed": 42
  }
}
```

- `packages`: number of packages (1 = single-package repo)
- `commits`: number of synthetic commits
- `seed`: optional RNG seed for deterministic output

Uses an incremental tree builder for fast generation (10k commits in under a minute).

### Advanced features

#### Tags at arbitrary commits

```json
{
  "tags": [
    { "name": "v1.0.0", "at_commit": -1 },
    { "name": "v1.1.0", "at_commit": 2 }
  ]
}
```

- `at_commit`: `-1` = initial setup commit, `0+` = index into `commits` array

The `tag` field on package entries still works for tags on the initial commit.

#### Config format selection

```json
{
  "config": {
    "format": "toml",
    "filename": "ferrflow.toml",
    "content": "..."
  }
}
```

- `format`: `"json"` (default), `"toml"`, or `"json5"`
- `filename`: optional, auto-derived from format if omitted

#### Hook scripts

```json
{
  "hooks": [
    {
      "path": "hooks/pre-bump.sh",
      "content": "#!/usr/bin/env bash\necho \"running pre-bump\"\n"
    }
  ]
}
```

#### Custom default branch

```json
{
  "meta": {
    "name": "my-fixture",
    "description": "Repo with master as default branch",
    "default_branch": "master"
  }
}
```

If omitted, the branch name depends on libgit2's compiled-in default (usually `master`). Always set `default_branch` explicitly if the branch name matters for your test.

#### Multiple branches

Create branches from specific points and optionally merge them back:

```json
{
  "branches": [
    {
      "name": "develop",
      "from": "main",
      "at_commit": 0,
      "merge": "main",
      "commits": [
        { "message": "feat: new feature on develop", "files": ["src/develop.rs"] },
        { "message": "fix: develop bugfix", "files": ["src/develop-fix.rs"] }
      ]
    },
    {
      "name": "feature/analytics",
      "from": "main",
      "commits": [
        { "message": "feat: add analytics", "files": ["src/analytics.rs"] }
      ]
    }
  ]
}
```

- `from`: source branch name (defaults to `default_branch`)
- `at_commit`: commit index to branch from (`-1` = initial setup, `0+` = index into `commits`). If omitted, branches from the tip of `from`.
- `merge`: if set, creates a merge commit back into the named branch after all commits are added
- `commits`: list of commits to add on this branch (same format as top-level `commits`)

#### Merge commits

```json
{
  "commits": [
    { "message": "feat: merged feature", "files": ["src/feature.rs"], "merge": true }
  ]
}
```

## Directory structure

```
.
├── action.yml                 # GitHub Action definition
├── generator/                 # Rust binary that builds fixture repos
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs            # Entry point
│       ├── cli.rs             # Argument parsing (--help, --version, etc.)
│       ├── generate.rs        # Fixture generation logic
│       ├── types.rs            # Definition structs and deserialization
│       ├── tree.rs            # Incremental tree builder for bulk generation
│       ├── rng.rs             # Deterministic RNG for synthetic commits
│       └── validate.rs        # Definition validation (--validate flag)
├── schema/
│   └── fixture.schema.json    # JSON Schema for fixture definitions
├── fixtures/
│   └── examples/              # Example definitions for reference
└── .github/
    └── workflows/
        ├── test.yml           # CI: fmt, clippy, test, build, generate
        └── validate.yml       # Reusable workflow for consumer repos
```

## License

[MPL-2.0](LICENSE)
