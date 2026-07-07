# Usage

## CLI

The binary is called `oboros`. Usage:

```
oboros [--config <FILE>] [--format human|json] [--package] [--dump-ignores] [--strict]
```

| Flag | Description |
|------|-------------|
| `--config <FILE>` | Path to an `oboros.toml` config file. If omitted, Ouroboros walks upward from the current directory to find one. |
| `--format <FORMAT>` | Output format: `human` (default) or `json`. When `json`, all verbose intermediate output is suppressed and a single JSON object is emitted to stdout. |
| `--package` | Only report cycles where all files belong to the same top-level package. Cross-package cycles are excluded. See [Intra-package filtering](#intra-package-filtering---package). |
| `--dump-ignores` | Print ignore entries for all detected cycles, then exit. With `--format human` (default), prints TOML fragments. With `--format json`, prints a JSON object. |
| `--strict` | Exit with code 1 if any (non-suppressed) cycles are detected. Works with both output formats. |

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

#### `[[cycles.ignore]]` entries

Suppress known cycles so they do not appear in output or trigger `--strict` failures. Each entry lists the exact set of files forming the cycle, with an optional `reason`.

```toml
[[cycles.ignore]]
files = ["pkg/a.py", "pkg/b.py"]
reason = "known cycle, tracked in PROJ-123"

[[cycles.ignore]]
files = ["pkg/x.py", "pkg/y.py", "pkg/z.py"]
reason = "refactor planned for Q3"
```

The `files` list must match a detected cycle exactly (same set of paths, order does not matter). If an ignore entry does not match any detected cycle, Ouroboros prints a warning to stderr.

Use `--dump-ignores` to bootstrap ignore entries from currently detected cycles:

```bash
# Print TOML fragments you can paste into oboros.toml
oboros --dump-ignores

# Or get JSON for scripting
oboros --dump-ignores --format json
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

SCCs that pass the configured size filter, grouped by top-level package. Each file shows the line numbers where cycle-participating imports occur.

```
--- dependency cycles (3) ---
(1 cycles suppressed by ignore list)

package: pkg (2 cycles)

cycle 1 (3 files)
  pkg/a.py (imports at lines 12, 45)
  pkg/b.py (import at line 8)
  pkg/c.py (import at line 3)

cycle 2 (2 files)
  pkg/x.py (import at line 5)
  pkg/y.py (import at line 11)

(cross-package: pkg, lib) (1 cycle)

cycle 3 (2 files)
  pkg/foo.py (import at line 7)
  lib/bar.py (import at line 14)
```

Cycles are sorted by package name, then by size. When `--package` is active, only intra-package cycles (single package group) are shown.

### JSON output (`--format json`)

When `--format json` is used, all verbose sections above are suppressed and a single JSON object is printed to stdout:

```json
{
  "version": 1,
  "summary": {
    "cycles_reported": 2,
    "cycles_suppressed": 1
  },
  "cycles": [
    {
      "index": 1,
      "packages": ["pkg"],
      "size": 3,
      "files": [
        {
          "path": "pkg/a.py",
          "import_lines": [12, 45],
          "edges": [
            { "to": "pkg/b.py", "lines": [12] },
            { "to": "pkg/c.py", "lines": [45] }
          ]
        }
      ]
    }
  ]
}
```

| Field | Type | Description |
|-------|------|-------------|
| `version` | integer | Schema version (always `1`). |
| `summary.cycles_reported` | integer | Number of cycles in the `cycles` array. |
| `summary.cycles_suppressed` | integer | Number of cycles suppressed by the ignore list. |
| `cycles[].index` | integer | 1-based cycle index. |
| `cycles[].packages` | array of strings | Sorted list of top-level packages involved in the cycle (e.g. `["pkg"]` for intra-package, `["lib", "pkg"]` for cross-package). |
| `cycles[].size` | integer | Number of files in the cycle. |
| `cycles[].files[].path` | string | Relative file path. |
| `cycles[].files[].import_lines` | array of integers | Sorted line numbers of imports to other cycle members. |
| `cycles[].files[].edges[].to` | string | Import target path within the cycle. |
| `cycles[].files[].edges[].lines` | array of integers | Sorted line numbers for that specific edge. |

Pipe to `jq` for filtering:

```bash
oboros --format json | jq '.cycles | length'
oboros --format json | jq '.cycles[] | select(.size > 3)'
```

Warnings and errors still go to stderr regardless of format.

---

## Intra-package filtering (`--package`)

By default, Ouroboros reports all cycles regardless of which packages the files belong to. The `--package` flag restricts output to cycles where every file shares the same top-level package directory.

A file's top-level package is its first path component (e.g. `pkg/sub/a.py` belongs to package `pkg`). Files at the root level (no subdirectory) have no package.

This is useful in large monorepos where cross-package cycles are tracked separately or owned by different teams, and you want to focus on cycles within a single package.

```bash
# Show only cycles internal to a single package
oboros --package

# Combine with --strict for CI: fail only on intra-package cycles
oboros --package --strict
```

---

## Practical examples

### CI gate: fail on any new cycles

```bash
oboros --strict
```

Exit code 1 if any non-suppressed cycles exist. Add `[[cycles.ignore]]` entries for known cycles to avoid false positives.

### Bootstrap an ignore list for an existing project

```bash
oboros --dump-ignores >> oboros.toml
```

Appends TOML `[[cycles.ignore]]` fragments for every detected cycle. Edit the `reason` fields, then future runs will suppress those cycles.

### JSON report filtered by package

```bash
oboros --format json --package | jq '.cycles[] | select(.size > 3)'
```

### Focus on small, actionable cycles within each package

```bash
oboros --package --strict
```

Combined with `max-scc-size` in config, this targets small intra-package tangles that are easiest to fix first.

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

The leading dot is interpreted with Python's package semantics: inside a package's `__init__.py`, a single dot refers to the package **itself**, whereas inside a regular module it refers to the module's **parent** package. For example, `from .staff import x`:

- in `pkg/services/__init__.py` (package `pkg.services`) resolves to `pkg.services.staff`
- in `pkg/services/api.py` (module `pkg.services.api`) also resolves to `pkg.services.staff`

Handling `__init__.py` correctly is required to detect cycles that close through an eager `__init__.py` re-export (a package `__init__` that imports its own submodules).

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
