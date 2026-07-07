# Ouroboros

A fast, Rust-powered CLI tool that detects **first-party circular imports** in Python projects.

Ouroboros indexes your Python source files, extracts import relationships, resolves them to first-party modules, builds a dependency graph, and uses [Tarjan's SCC algorithm](https://en.wikipedia.org/wiki/Tarjan%27s_strongly_connected_components_algorithm) to find circular dependencies — all without running Python.

Designed for large monorepos with millions of lines of code.

## Features

- Discovers `.py` files across configurable source roots
- Extracts `import` and `from ... import` statements (including relative imports)
- Resolves imports against a first-party module index
- Accounts for ancestor package `__init__.py` execution (importing `a.b.c` also depends on `a` and `a.b`); toggle with `include-ancestor-init` / `--no-include-ancestor-init`
- Builds a compact file-level dependency graph
- Detects circular dependencies via strongly connected components (SCCs)
- Configurable SCC size filtering, local-import inclusion, and source roots
- Human-readable output with cycles grouped by package and import line numbers
- JSON output (`--format json`) for programmatic consumption
- Ignore list support (`[[cycles.ignore]]` in config) to suppress known cycles
- `--dump-ignores` to bootstrap ignore lists from detected cycles
- `--strict` mode for CI enforcement (exit code 1 on cycles)
- `--package` flag to filter to intra-package cycles only

See [USAGE.md](USAGE.md) for full details on every flag and config option.

## Installation

### From source (Rust)

Requires [Rust 1.85+](https://rustup.rs/).

```bash
cargo install --path crates/ouroboros-cli
```

The binary is called `oboros`.  

### As a Python wheel

Requires [maturin](https://www.maturin.rs/) and Python 3.8+.

```bash
maturin build --release
pip install target/wheels/ouroboros-*.whl
```

## Quick start

1. Create an `oboros.toml` in your Python project root:

```toml
source-roots = ["src"]
```

2. Run it:

```bash
oboros
```

Ouroboros automatically discovers `oboros.toml` by walking upward from the current directory. You can also point to a config explicitly:

```bash
oboros --config path/to/oboros.toml
```

All CLI flags:

```
oboros [--config <FILE>] [--format human|json] [--package] [--dump-ignores] [--strict] [--no-include-ancestor-init]
```

See [USAGE.md](USAGE.md) for the full configuration reference and detailed usage instructions.

## Project structure

```
ouroboros/
├── crates/
│   ├── ouroboros-cli/    # CLI binary (oboros)
│   └── ouroboros-core/   # Core library
│       └── src/
│           ├── config.rs       # Config loading & validation
│           ├── discovery/      # File discovery & module indexing
│           ├── parser/         # Python import extraction
│           ├── resolver/       # Import-to-module resolution
│           ├── graph/          # Dependency graph & SCC detection
│           └── cycles/         # Cycle filtering & reporting
├── fixtures/
│   ├── generate.py             # Test fixture generator
│   └── sample_project/         # Generated sample project (git-ignored)
└── pyproject.toml              # Python wheel build config
```

## How it works

Ouroboros runs through six phases:

1. **Discovery** — walks configured source roots, finds `.py` files, and maps each to a canonical module name (e.g. `src/pkg/a.py` → `pkg.a`)
2. **Import extraction** — parses each file with [RustPython](https://github.com/RustPython/Parser) and extracts import statements
3. **Resolution** — resolves raw imports against the first-party module index, classifying each as resolved, unresolved, or ambiguous
4. **Graph building** — constructs a directed dependency graph from resolved edges
5. **Cycle detection** — runs Tarjan's algorithm to find strongly connected components
6. **Reporting** — filters SCCs by configured size bounds and prints results

## Development

```bash
cargo build              # build all crates
cargo test               # run tests
cargo run -p ouroboros-cli -- --config fixtures/sample_project/oboros.toml  # run against fixtures
```

### Generating test fixtures

The fixture generator creates a sample Python project with known circular imports:

```bash
python fixtures/generate.py                # default (~30 files)
python fixtures/generate.py --scale 10     # larger project (~280 files)
python fixtures/generate.py --seed 123     # reproducible randomness
```

## License

TBD
