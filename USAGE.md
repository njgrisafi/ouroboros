# Usage

## CLI

The binary is called `oboros`. It accepts a single optional flag:

```
oboros [--config <FILE>]
```

| Flag | Description |
|------|-------------|
| `--config <FILE>` | Path to an `oboros.toml` config file. If omitted, Ouroboros walks upward from the current directory to find one. |

If no config file is found, built-in defaults are used (source root: `src`, top-level imports only, minimum SCC size: 2).

### Examples

Run in a project that has `oboros.toml` at its root:

```bash
cd my-python-project
oboros
```

Point to a specific config:

```bash
oboros --config /path/to/my-project/oboros.toml
```

---

## Configuration

Ouroboros is configured via an `oboros.toml` file placed at the root of your Python project. All paths in the config are relative to the directory containing the config file.

### Minimal example

```toml
source-roots = ["src"]
```

### Full example

```toml
source-roots = ["src", "lib"]

[parse]
local-imports = true

[cycles]
min-scc-size = 2
max-scc-size = 10
```

### Reference

#### `source-roots` (required)

A list of directories containing first-party Python source code, relative to the project root.

```toml
source-roots = ["src"]
source-roots = ["src", "lib", "packages/core"]
source-roots = ["."]     # project root is the source root
```

Each source root is walked recursively for `.py` files. The module name for each file is derived from its path relative to the source root (e.g. `src/pkg/a.py` under source root `src` becomes module `pkg.a`).

**Default (when no config is found):** `["src"]`

#### `[parse]` section

Controls how Python imports are extracted from source files.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `local-imports` | `bool` | `false` | Whether to include imports nested inside functions, methods, classes, and control-flow blocks. When `false`, only top-level imports are considered. |

Setting `local-imports = true` is useful when your codebase uses deferred imports (e.g. inside functions) to break runtime cycles, and you want to detect those hidden dependencies too.

```toml
[parse]
local-imports = true
```

#### `[cycles]` section

Controls which strongly connected components (SCCs) are reported.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `min-scc-size` | `integer` | `2` | Minimum number of files in an SCC for it to be reported. Must be at least 1. |
| `max-scc-size` | `integer` | _(none)_ | Maximum SCC size to report. If omitted, no upper bound is applied. Must be >= `min-scc-size`. |

A value of `min-scc-size = 1` will also report self-cycles (a file that imports itself). The default of `2` skips self-cycles and only reports groups of two or more files that form a cycle.

Use `max-scc-size` to focus on small, actionable cycles and exclude massive tangles:

```toml
[cycles]
min-scc-size = 2
max-scc-size = 5
```

---

## Output

Ouroboros prints its results to stdout in several sections:

### Source roots

Lists each configured source root and the `.py` files discovered within it, along with their resolved module names.

```
source root: /path/to/src (42 files)
  pkg/__init__.py -> pkg
  pkg/a.py -> pkg.a
  pkg/b.py -> pkg.b
  ...
```

### Imports

Shows the imports extracted from each file:

```
--- imports ---

  pkg.a:
    import pkg.b ()
    from   pkg (c)
```

### Resolved first-party dependencies

The edges in the first-party dependency graph:

```
--- resolved first-party dependencies (15) ---
  pkg.a -> pkg.b
  pkg.b -> pkg.c
  ...
```

### Unresolved imports

Imports that could not be resolved to a first-party module (typically stdlib or third-party):

```
--- unresolved imports (8) ---
  pkg.a -> os
  pkg.a -> typing
  ...
```

### Dependency graph

The full adjacency list of the file-level dependency graph:

```
--- dependency graph ---

pkg/__init__.py
  -> pkg/a.py
pkg/a.py
  -> pkg/b.py
  -> pkg/c.py
```

### Dependency cycles

SCCs that pass the configured size filter:

```
--- dependency cycles (2) ---

cycle 1 (3 files)
  pkg/a.py
  pkg/b.py
  pkg/c.py

cycle 2 (2 files)
  pkg/x.py
  pkg/y.py
```

---

## Import resolution rules

Understanding how Ouroboros resolves imports helps interpret the results.

### `import a.b.c`

Looks up the exact module `a.b.c` in the first-party index. If it exists, an edge is added. Otherwise the import is marked unresolved.

### `from a.b import c`

1. First tries `a.b.c` as a submodule
2. If that exists, the edge points to the file owning `a.b.c`
3. Otherwise falls back to `a.b`
4. If neither exists, the import is marked unresolved

### Relative imports

Relative imports (`from . import x`, `from ..foo import bar`) are converted to absolute module paths based on the importing file's own module name, then resolved using the rules above.

### `__init__.py` ownership

- `pkg/__init__.py` owns the module `pkg`
- `pkg/mod.py` owns the module `pkg.mod`

---

## Fixture generator

The repository includes a fixture generator for testing at `fixtures/generate.py`. It produces a sample Python project under `fixtures/sample_project/` with known circular import patterns.

```bash
python fixtures/generate.py [--scale N] [--seed N]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--scale N` | `1` | Scale factor. Base skeleton is ~30 files; each increment adds ~25 more. |
| `--seed N` | `42` | Random seed for reproducible generation. |

The generated project includes an `oboros.toml` and can be used directly:

```bash
python fixtures/generate.py --scale 5
oboros --config fixtures/sample_project/oboros.toml
```

The `fixtures/sample_project/` directory is git-ignored.
