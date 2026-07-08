# Usage

## CLI

The binary is called `oboros`. Usage:

```
oboros [--config <FILE>] [--format human|json] [--trace <PATH>] [--package] [--dump-ignores] [--strict] [--no-include-ancestor-init]
```

| Flag | Description |
|------|-------------|
| `--config <FILE>` | Path to an `oboros.toml` config file. If omitted, Ouroboros walks upward from the current directory to find one. |
| `--format <FORMAT>` | Output format: `human` (default) or `json`. When `json`, all verbose intermediate output is suppressed and a single JSON object is emitted to stdout. |
| `--package` | Only report cycles where all files belong to the same top-level package. Cross-package cycles are excluded. See [Intra-package filtering](#intra-package-filtering---package). |
| `--dump-ignores` | Print ignore entries for all detected cycles, then exit. With `--format human` (default), prints TOML fragments. With `--format json`, prints a JSON object. |
| `--strict` | Exit with code 1 if any (non-suppressed) cycles are detected. When `--trace` is also present, exits 1 only if the union of impacting cycles across all traced paths is non-empty. Works with both output formats. |
| `--no-include-ancestor-init` | Disable ancestor-package `__init__.py` edges. Overrides `include-ancestor-init` in config. See [`[resolve]` section](#resolve-section). |
| `--trace <PATH>`, `-t <PATH>` | Report cycles that impact the given file or directory path(s), relative to a source root. Repeatable and/or comma-separated. When omitted, output is identical to today. See [Cycle impact](#cycle-impact---trace). |

If no config file is found, built-in defaults are used (source root: `src`, top-level imports only, minimum SCC size: 2, ancestor `__init__.py` edges enabled).

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

[resolve]
include-ancestor-init = true

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

#### `[resolve]` section

Controls how resolved imports are turned into dependency edges.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `include-ancestor-init` | `bool` | `true` | Whether to also record dependency edges to the `__init__.py` of every first-party ancestor package of an imported module. |

Importing `a.b.c` causes Python to execute `a/__init__.py` and `a/b/__init__.py` at import time, so those ancestor packages are genuine import-time dependencies. When `include-ancestor-init = true` (the default), Ouroboros records edges to them. This surfaces real cycles that close through an eager parent `__init__.py` — for example when `beta/helpers.py` imports `alpha.core`, which executes `alpha/__init__.py`, which in turn re-exports something from `beta`.

Edges are **not** recorded to the importing module's *own* ancestor packages. When `alpha.sub.mod` imports a sibling, `alpha` and `alpha.sub` are already initialized on `alpha.sub.mod`'s own import path, so no `alpha.sub.mod -> alpha` edge is added. This is what prevents false cycles when a package `__init__.py` re-exports one of its submodules (the submodule importing another sibling does not re-enter the parent).

Set `include-ancestor-init = false` (or pass `--no-include-ancestor-init`) to restrict edges to the deepest resolved module only, matching the pre-1.0 behavior. The CLI flag takes precedence over the config value.

```toml
[resolve]
include-ancestor-init = false
```

Enabling this option may increase the number of reported cycles, since it exposes previously-hidden latent cycles. Passive `__init__.py` files (those with no first-party imports of their own) can be edge targets but can never be part of a cycle, so they do not produce false positives.

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

---

## Cycle impact (`--trace`)

The `--trace` flag lets you ask: *"which cycles affect this file or directory?"* It reports every import cycle that **impacts** a given path — either because the path is a direct member of the cycle, or because the path's import chain leads into the cycle.

### What "impact" means

A cycle **impacts** a traced file `T` if:
- **Member** — `T` is part of the cycle (direct participation), or
- **Reachable** — `T`'s import chain leads into the cycle (`T → … → cycle member`)

For reachable impacts, Ouroboros reports the **shortest import path** from `T` to the cycle, annotated with the exact line numbers of each import statement.

### Usage

```bash
# Trace a single file
oboros --trace app/entry.py

# Short alias
oboros -t app/entry.py

# Trace a directory (all .py files under it)
oboros --trace app/

# Trace multiple paths (comma-separated or repeated flag)
oboros --trace app/entry.py,app/mid.py
oboros --trace app/entry.py --trace app/mid.py

# Combine with --format json for programmatic use
oboros --format json --trace app/

# Source-root prefix is stripped automatically
oboros --trace src/app/entry.py   # same as --trace app/entry.py when source-roots = ["src"]
```

### Human output

When `--trace` is used, a `--- cycle impact ---` section is appended after the dependency cycles section:

```
--- cycle impact ---

trace: app/ (directory, 4 of 6 files impacted)
  app/core_a.py:
    impacted by 1 cycle:
      cycle 1 (member)
  app/core_b.py:
    impacted by 1 cycle:
      cycle 1 (member)
  app/entry.py:
    impacted by 1 cycle:
      cycle 1 (reachable via app/entry.py:1 -> app/mid.py:1 -> app/core_a.py)
  app/mid.py:
    impacted by 1 cycle:
      cycle 1 (reachable via app/mid.py:1 -> app/core_a.py)

trace: app/isolated.py (file)
  not impacted by any cycle

(unknown paths: does/not/exist.py)
```

- **Directory traces** show `N of M files impacted` and list only impacted files (clean files are suppressed from the human output but still appear in JSON).
- **File traces** show `not impacted by any cycle` when clean.
- **Unknown paths** (no matching graph nodes) are listed at the end and warned to stderr.
- The `cycle N` numbers match the numbers in the dependency cycles section above.

### JSON output

When `--format json --trace` is used, two optional top-level fields are added:

```json
{
  "version": 1,
  "summary": { "cycles_reported": 1, "cycles_suppressed": 0 },
  "cycles": [ ... ],
  "traced": [
    {
      "path": "app/entry.py",
      "kind": "file",
      "files": [
        {
          "path": "app/entry.py",
          "impacts": [
            {
              "cycle_index": 1,
              "relationship": "reachable",
              "entry": "app/core_a.py",
              "from_lines": [1],
              "path": [
                { "from": "app/entry.py", "to": "app/mid.py", "lines": [1] },
                { "from": "app/mid.py",   "to": "app/core_a.py", "lines": [1] }
              ]
            }
          ]
        }
      ]
    }
  ],
  "unknown_paths": ["does/not/exist.py"]
}
```

These fields are **omitted** when `--trace` is not used, so existing consumers are unaffected.

| Field | Type | Description |
|-------|------|-------------|
| `traced[].path` | string | The traced path as given (directory paths end with `/`). |
| `traced[].kind` | string | `"file"` or `"directory"`. |
| `traced[].files[].path` | string | Graph node path (matches `cycles[].files[].path`). |
| `traced[].files[].impacts` | array | Omitted when empty (file is clean). |
| `traced[].files[].impacts[].cycle_index` | integer | Matches `cycles[].index`. |
| `traced[].files[].impacts[].relationship` | string | `"member"` or `"reachable"`. |
| `traced[].files[].impacts[].entry` | string | First cycle member reached. |
| `traced[].files[].impacts[].from_lines` | array of integers | Import line(s) in the traced file that begin the branch. Omitted for `"member"`. |
| `traced[].files[].impacts[].path` | array of hops | Import chain from traced file to cycle entry. Omitted for `"member"`. |
| `traced[].files[].impacts[].path[].from` | string | Importing file. |
| `traced[].files[].impacts[].path[].to` | string | Imported file (next toward the cycle). |
| `traced[].files[].impacts[].path[].lines` | array of integers | Import line numbers for this edge. |
| `unknown_paths` | array of strings | Paths that matched no graph nodes. Omitted when empty. |

### Impact-scoped `--strict`

When `--trace` is present, `--strict` exits 1 only if the union of impacting cycles across all traced paths is non-empty:

```bash
# Exit 1 if app/entry.py is impacted by any cycle
oboros --trace app/entry.py --strict

# Exit 0 if app/isolated.py is not impacted (even if other cycles exist)
oboros --trace app/isolated.py --strict
```

Without `--trace`, `--strict` behaves as before (exits 1 if any non-suppressed cycles exist).

### `--dump-ignores` interaction

`--trace` is a no-op when `--dump-ignores` is used. The dump-ignores output is always whole-project.

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

### Trace a file's cycle impact

```bash
# Find all cycles that affect app/entry.py
oboros --trace app/entry.py

# CI: fail only if app/entry.py is impacted by a cycle
oboros --trace app/entry.py --strict

# Trace an entire directory
oboros --trace app/ --format json | jq '.traced[0].files[] | select(.impacts != null)'
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

The leading dot is interpreted with Python's package semantics: inside a package's `__init__.py`, a single dot refers to the package **itself**, whereas inside a regular module it refers to the module's **parent** package. For example, `from .staff import x`:

- in `pkg/services/__init__.py` (package `pkg.services`) resolves to `pkg.services.staff`
- in `pkg/services/api.py` (module `pkg.services.api`) also resolves to `pkg.services.staff`

Handling `__init__.py` correctly is required to detect cycles that close through an eager `__init__.py` re-export (a package `__init__` that imports its own submodules).

### Ancestor package `__init__.py` edges

Whenever an import resolves to a first-party module, Ouroboros (by default) also records edges to every first-party ancestor package on the path. Importing `a.b.c` executes `a/__init__.py` and `a/b/__init__.py` at import time, so `a` and `a.b` are treated as dependencies of the importing file too. Ancestor packages that already **contain** the importing module are skipped (they are guaranteed initialized before the module runs), so importing a sibling never adds an edge back to a shared parent package. This is controlled by [`include-ancestor-init`](#resolve-section) and can be disabled with `--no-include-ancestor-init`.

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
