# FerrFlow Tests

Integration test fixtures and CI for [FerrFlow](https://github.com/FerrFlow-Org/FerrFlow).

## How it works

1. **Fixture definitions** (`fixtures/definitions/`) describe test scenarios declaratively in TOML: packages, commits, tags, config, and expected outputs
2. **Generator** (`generator/`) reads definitions and builds real git repos with precise histories
3. **Runner** (`scripts/run-tests.sh`) executes `ferrflow check` against each generated repo and compares output to snapshots
4. **CI** runs on every push and on a schedule, also triggered by FerrFlow PRs via a reusable workflow

## Directory structure

```
.
├── fixtures/
│   ├── definitions/       # TOML files describing test scenarios
│   └── generated/         # (gitignored) repos built by the generator
├── generator/             # Rust binary that builds fixture repos from definitions
│   ├── Cargo.toml
│   └── src/
│       └── main.rs
├── snapshots/             # Expected CLI output per fixture
├── scripts/
│   └── run-tests.sh       # Test runner script
└── .github/
    └── workflows/
        ├── test.yml       # CI: generate fixtures and run tests
        └── action.yml     # Reusable action for FerrFlow CI
```

## Fixture definition format

Each `.toml` file in `fixtures/definitions/` describes a scenario:

```toml
[meta]
name = "monorepo-two-packages"
description = "Two packages with independent version bumps"

[config]
# Inline ferrflow.json content
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

## Running locally

```bash
# Build the generator
cd generator && cargo build --release

# Generate all fixtures
../target/release/generate-fixtures

# Run tests (requires ferrflow in PATH)
./scripts/run-tests.sh
```

## Adding a new test

1. Create a new `.toml` file in `fixtures/definitions/`
2. Add expected output in `snapshots/<fixture-name>.txt` (or let CI generate it on first run)
3. Push — CI handles the rest

## Reusable generator

The fixture generator is designed to be reusable across the FerrFlow ecosystem. Any repo that needs realistic git repos with precise histories can use it:

- **Tests** (this repo) — generate fixtures and run `ferrflow check` against them
- **Benchmarks** (`FerrFlow-Org/Benchmarks`) — generate repos at various scales for perf testing
- **Playground** — generate demo repos for the web playground

The generator reads declarative TOML definitions and produces real git repos. No shell scripts, no manual setup.

## License

MIT
