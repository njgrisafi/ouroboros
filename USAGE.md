# Usage

## CLI

The binary is called `oboros`. Usage:

```
oboros [--config <FILE>] [--format human|json] [--trace <PATH>] [--package] [--dump-ignores] [--dump-cyclic-files] [--check-cyclic-files] [--check-max-cycles] [--max-cycles <N>] [--show-cyclic-files] [--ignore-derived-ancestor-init] [--include-self-ancestor-init] [--local-imports] [--include-string-imports] [--strict] [--no-include-ancestor-init] [--exclude <PATH>] [--write]
```

| Flag | Description |
|------|-------------|
| `--config <FILE>` | Path to an `oboros.toml` config file. If omitted, Ouroboros walks upward from the current directory to find one. |
| `--format <FORMAT>` | Output format: `human` (default) or `json`. When `json`, all verbose intermediate output is suppressed and a single JSON object is emitted to stdout. |
| `--package` | Only report cycles where all files belong to the same top-level package. Cross-package cycles are excluded. See [Intra-package filtering](#intra-package-filtering---package). |
| `--dump-ignores` | Print ignore entries for all detected cycles, then exit. With `--format human` (default), prints TOML fragments. With `--format json`, prints a JSON object. Use with `--write` to patch `oboros.toml` directly. |
| `--dump-cyclic-files` | Print the sorted set of files participating in any cycle as a pasteable TOML fragment (human) or JSON object (`--format json`), then exit. Use with `--write` to patch `oboros.toml` directly. |
| `--check-cyclic-files` | Compare `[cycles] cyclic-files` in config against the freshly-computed set; exit 0 if identical, exit 1 if any difference (with a human diff on stderr). Independent of `--format`. Short-circuits the normal report. |
| `--check-max-cycles` | Fail (exit 1) if the number of detected cycles exceeds a budget. The budget comes from `--max-cycles` or `[cycles] max-cycles`. Exit 2 if no budget is set or if combined with `--check-cyclic-files`. Short-circuits the normal report. |
| `--max-cycles <N>` | Budget on the number of cycles allowed (e.g. `9` = allow up to 9 cycles, fail on the 10th). Overrides `[cycles] max-cycles` in config. Only meaningful with `--check-max-cycles`. |
| `--show-cyclic-files` | Include the cyclic-files set as an optional top-level `cyclic_files` array in the JSON report. No-op in human mode. |
| `--ignore-derived-ancestor-init` | Exclude files that are cyclic only via a derived ancestor-`__init__.py` edge from the cyclic-files baseline. Overrides `[cycles] ignore-derived-ancestor-init` in config. Baseline-only; does not affect the normal cycle report. |
| `--include-self-ancestor-init` | Detect cycles that close through an eager parent `__init__.py`. Overrides `[resolve] include-self-ancestor-init` in config. See [`[resolve]` section](#resolve-section). |
| `--local-imports` | Include imports nested inside functions, methods, classes, and control-flow blocks (deferred/"local" imports), not just top-level ones. Overrides `[parse] local-imports` in config. See [`[parse]` section](#parse-section). |
| `--include-string-imports` | Also treat string literals that look like module paths as imports (ruff-style "string imports", e.g. `importlib.import_module("a.b.c")`). Respects `--local-imports` for whether strings nested inside functions/classes are scanned. Overrides `[parse] string-imports` in config. See [`[parse]` section](#parse-section). |
| `--strict` | Exit with code 1 if any (non-suppressed) cycles are detected. When `--trace` is also present, exits 1 only if the union of impacting cycles across all traced paths is non-empty. Works with both output formats. |
| `--no-include-ancestor-init` | Disable ancestor-package `__init__.py` edges. Overrides `include-ancestor-init` in config. See [`[resolve]` section](#resolve-section). |
| `--trace <PATH>`, `-t <PATH>` | Report cycles that impact the given file or directory path(s), relative to the project root. Repeatable and/or comma-separated. When omitted, output is identical to today. See [Cycle impact](#cycle-impact---trace). |
| `--exclude <PATH>` | Exclude paths (files or directories) from analysis seeds. Repeatable and/or comma-separated. Unioned with `exclude` in config. See [Exclude paths](#exclude-paths). |
| `--write` | Write the dump output directly into `oboros.toml` instead of printing to stdout. Requires `--dump-cyclic-files` or `--dump-ignores`. Patches the file in-place, preserving comments and formatting. Errors (exit 2) if no config file is found. |

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
string-imports = true

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
| `string-imports` | `bool` | `false` | Whether to detect string literals that name modules (dynamic "string imports") and treat them as dependency edges. Respects the `local-imports` nesting gate (see below). |
| `string-imports-mode` | `"call-sites"` \| `"all"` | `"call-sites"` | Which strings count as candidates. `"call-sites"` only scans first arguments of `importlib.import_module(...)` / `import_module(...)` / `__import__(...)` calls and resolves them exactly — precise, suited to cycle detection. `"all"` scans every string literal that looks like a dotted path (ruff's `analyze.detect-string-imports` parity) — aggressive, suited to dependency-graph use cases. |
| `string-imports-min-dots` | `int` | `2` | Minimum number of dots a string literal must contain to be considered a module-path candidate. Only applies in `"all"` mode (ignored in `"call-sites"`, where exact resolution is the precision mechanism). Matches ruff's `string-imports-min-dots` default. `0` disables the dot requirement (very aggressive). |

Setting `local-imports = true` is useful when your codebase uses deferred imports (e.g. inside functions) to break runtime cycles, and you want to detect those hidden dependencies too.

```toml
[parse]
local-imports = true
```

You can also enable this for a single run without editing the config by passing `--local-imports` on the CLI. The flag forces the option on and takes precedence over the config value (which defaults to `false`).

#### String imports

Some codebases load modules dynamically by name — `importlib.import_module("a.b.c")`, plugin loaders — and those edges are invisible to statement-level import extraction. With `string-imports` enabled, Ouroboros treats such strings as import candidates. Two modes control how aggressive this is:

**`call-sites` mode (default).** Only string literals passed as the first argument of `importlib.import_module(...)`, `import_module(...)`, or `__import__(...)` count. Candidates must resolve **exactly** to a first-party module or package — there is no prefix shortening, because `import_module("a.b.c.MyClass")` would raise `ModuleNotFoundError` at runtime anyway. `string-imports-min-dots` is ignored in this mode. This is the mode to use for cycle detection.

**`all` mode (ruff parity).** Like ruff's `analyze.detect-string-imports`, **every** string literal is scanned — including registry-style strings in Django settings, DI containers, task-name constants, and `mock.patch` targets:

- The string must have at least `string-imports-min-dots` dots (default 2, matching ruff/Pants) and consist of dot-separated identifier segments. Byte strings (`b"..."`) never count; f-string literal fragments don't count, but strings inside f-string interpolations do; docstrings are scanned like any other string.
- Trailing components may be attributes, so the resolver tries progressively shorter prefixes: `"a.b.c.MyClass"` resolves to `a.b.c`. Prefixes with fewer than `string-imports-min-dots` dots are never tried.
- Warning: on large codebases with module-path registries, `all` mode can fuse much of the graph into one giant SCC. Prefer it for dependency-graph/impact analysis, not cycle gating.

In both modes:

- Candidates that match no first-party module are **dropped silently** — they are not reported as unresolved imports and never trip `--strict`. A module string-importing itself (e.g. `importlib.import_module(__name__)`) is also dropped.
- String imports compose with `local-imports`: when `local-imports` is off (default), only module-level strings are scanned; when on, strings inside functions, classes, and control-flow blocks are scanned too. Module-level `def` default arguments and decorators evaluate at import time, so strings there count as module-level.

```toml
[parse]
string-imports = true
string-imports-mode = "call-sites"
```

You can also enable string imports for a single run with `--include-string-imports`, which forces the option on and takes precedence over the config value.

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

##### `[resolve] include-self-ancestor-init`

When `true`, Ouroboros detects real cycles that close through an eager parent `__init__.py` — for example `P/__init__.py → P.child → P` — which the default `is_ancestor_or_self` guard suppresses. Only cycle-closing suppressed edges are restored; non-cyclic real ancestor edges are not added.

```toml
[resolve]
include-self-ancestor-init = true
```

**CLI flag:** `--include-self-ancestor-init` (no short form)

**Default:** `false` (opt-in). The tool's default output is unchanged.

**Interaction with `include-ancestor-init`:** This option has no effect when `include-ancestor-init` is disabled. A warning is emitted if both are misconfigured.

**Interaction with `ignore-derived-ancestor-init`:** Newly surfaced cycles are ancestor-init-derived and can be grandfathered via `ignore-derived-ancestor-init` or suppressed via `[[cycles.ignore]]` / `cyclic-files`.

**`min-scc-size` caveat:** Surfaced cycles are subject to `[cycles] min-scc-size` (default 2). Newly-surfaced self-tree init cycles are typically 2 files, so `min-scc-size ≥ 3` will hide them. If the flag appears to have no effect, check this setting.

#### `[cycles]` section

Controls which strongly connected components (SCCs) are reported.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `min-scc-size` | `integer` | `2` | Minimum number of files in an SCC for it to be reported. Must be at least 1. |
| `max-scc-size` | `integer` | _(none)_ | Maximum SCC size to report. If omitted, no upper bound is applied. Must be >= `min-scc-size`. |
| `max-cycles` | `integer` | _(none)_ | Budget on the number of cycles allowed (e.g. `9` = allow up to 9, fail on the 10th). Only enforced with `--check-max-cycles`; overridden by `--max-cycles`. |

A value of `min-scc-size = 1` will also report self-cycles (a file that imports itself). The default of `2` skips self-cycles and only reports groups of two or more files that form a cycle.

Use `max-scc-size` to focus on small, actionable cycles and exclude massive tangles:

```toml
[cycles]
min-scc-size = 2
max-scc-size = 5
```

#### Capping the number of cycles (`max-cycles` + `--check-max-cycles`)

`max-cycles` is a budget on the **number** of cycles (not their size). It is only enforced when `--check-max-cycles` is passed, which makes it a CI ratchet: record how many cycles you have today, and fail the build if a new one appears.

```toml
[cycles]
max-cycles = 9
```

```bash
# Fails (exit 1) if more than 9 cycles are detected
oboros --check-max-cycles

# Override the configured budget ad hoc
oboros --check-max-cycles --max-cycles 9
```

The count is taken from the final filtered cycle list (after `min-scc-size`/`max-scc-size`, `[[cycles.ignore]]`, `ignore-dirs`, and `--package`) — exactly the cycles a normal report would show. Running `--check-max-cycles` with no budget set in either place is a usage error (exit 2), as is combining it with `--check-cyclic-files`. Because the check short-circuits the normal report, `--strict` has no effect alongside `--check-max-cycles`.

**How this differs from the other cycle-gating flags:**

| Flag | What it checks | Tolerates churn in existing cycles? |
|------|---------------|--------------------------------------|
| `--strict` | any cycle present (budget of 0) | no |
| `--check-max-cycles` | cycle **count** exceeds budget | **yes** — files can enter/leave the cyclic set |
| `--check-cyclic-files` | the exact **set** of cyclic files changes | no |

Use `--check-max-cycles` when you are actively refactoring existing cycles and want to prevent new ones without freezing the file set. Use `--check-cyclic-files` when you want to freeze the set entirely. Use `--strict` when you have zero cycles and want to keep it that way.

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

#### `[cycles] ignore-dirs`

Suppress cycles that are entirely contained within specified directories. A cycle is dropped if every file in it is under one of the ignored directories; cycles with any file outside the ignored dirs are still reported.

```toml
[cycles]
ignore-dirs = ["app/protos/", "app/migrations/"]
```

**Semantics:**

- **Entries are project-root-relative** (same form as `exclude`, e.g. `["app/protos/", "app/migrations/"]`). Matching mirrors `exclude`: an entry matches a file exactly or matches any file under it as a directory prefix.
- **The decision is based purely on cycle membership, not on who imports the cycle:** if every file in a detected cycle is under one of the ignored directories, the whole cycle is dropped — even if non-ignored code imports into it. This is the opposite of `exclude`, which keys off import reachability from seeds.
- **A cycle is dropped entirely if every one of its files is under one of the ignored directories.** A cycle with any file outside the ignored dirs is still reported. This is the key difference from `exclude`: real cycles that cross from your code into generated directories are preserved.
- **Dropped cycles are removed everywhere:** not shown in human or JSON output, do not trigger `--strict`, and are excluded from `--dump-cyclic-files` / `--check-cyclic-files` / the `cyclic_files` set.

**Contrast with neighbors:**

- `exclude` = removes files from the analysis seed set (mypy-style); excluded files still get reported if a non-excluded file imports them, so `exclude` cannot hide a generated-internal cycle that your code reaches.
- `[[cycles.ignore]]` = suppresses one exact set of files (must match the cycle exactly).
- `ignore-dirs` = suppresses any cycle contained within the named directories, no need to enumerate file sets.

**Worked examples:**

*Example 1: cycle fully inside an ignored directory (suppressed)*

Config:
```toml
source-roots = ["app"]

[cycles]
ignore-dirs = ["app/protos/"]
```

Topology: two generated protobuf stubs import each other:
```
app/protos/scheduling/common/v1/__init__.py ⇄ app/protos/scheduling/v1/__init__.py
```

Both files are under `app/protos/`, so the entire cycle is dropped. Even though your application code imports `protos.scheduling.v1`, the cycle is removed from the report, from `--strict`, and from `--dump-cyclic-files`. Human output shows:
```
--- dependency cycles (0) ---
(1 cycles ignored by ignore-dirs)
```

*Example 2: cross-boundary cycle (still reported)*

Same config (`ignore-dirs = ["app/protos/"]`). Now a cycle routes through your own code:
```
app/services/a.py → app/protos/gen/x.py → app/services/b.py → app/services/a.py
```

This strongly connected component contains `app/services/a.py` and `app/services/b.py`, which are outside `app/protos/`. Because not every member is under an ignored directory, the cycle is still reported. `ignore-dirs` only hides cycles that are entirely generated-internal; it never hides real cycles that involve your hand-written code. Human output shows:
```
--- dependency cycles (1) ---
cycle 1 (3 files)
  app/services/a.py (import at line 5)
  app/protos/gen/x.py (import at line 12)
  app/services/b.py (import at line 8)
```

**Common use case:** generated code you don't own (betterproto stubs, database migrations, etc.):

```toml
[cycles]
ignore-dirs = ["app/protos/", "app/migrations/"]
```

**Validation:** entries must be relative (absolute paths and empty strings are rejected), consistent with `exclude`.

#### `[cycles] cyclic-files`

A sorted list of first-party files known to participate in an import cycle. Used with `--check-cyclic-files` to grandfather currently-cyclic files and block new ones from being introduced.

```toml
[cycles]
cyclic-files = [
    "pkg/a.py",
    "pkg/b.py",
]
```

**Definition:** the set contains every file that appears in any detected cycle, computed from the size-filtered cycles (respecting `min-scc-size`/`max-scc-size`) on the exclude-pruned graph. It is:
- **Independent of `[[cycles.ignore]]`**: a suppressed cycle's files still count as cyclic.
- **Independent of `--package`**: `--package` is a display filter only.
- **Dependent on `exclude`/`--exclude`** and `include-ancestor-init`: these change the analyzed graph.
- **Dependent on `min-scc-size`/`max-scc-size`**: re-run `--dump-cyclic-files` after changing size bounds.

> **Note:** In this initial version, files pulled into a cycle via ancestor-`__init__.py` edges (the `include-ancestor-init` mechanism) are counted. This is now available via `[cycles] ignore-derived-ancestor-init` (see below).

**Validation:** empty-string entries and absolute paths are rejected with an error.

#### Generating the list

```bash
# Write directly into oboros.toml (recommended — preserves comments and formatting)
oboros --dump-cyclic-files --write

# Or print a pasteable TOML fragment (human mode)
oboros --dump-cyclic-files
# Paste the output into [cycles] cyclic-files in oboros.toml
```

The human output includes a comment and the `[cycles]` header. If you already have a `[cycles]` table and prefer not to use `--write`, paste only the `cyclic-files = [...]` array into it — a blind `>> oboros.toml` redirect is only safe when no prior `[cycles]` table exists.

#### Checking for changes (`--check-cyclic-files`)

```bash
oboros --check-cyclic-files
```

Compares the configured `cyclic-files` list against the freshly-computed set:
- **Exit 0** — sets are identical; prints `cyclic files unchanged (N files)` to stderr.
- **Exit 1** — sets differ; prints a diff to stderr and a hint to re-run `--dump-cyclic-files`.

The diff format:
```
cyclic files changed:
  + pkg/new.py        (newly cyclic)
  - pkg/old.py        (no longer cyclic)
run `oboros --dump-cyclic-files --write` to update [cycles] cyclic-files in oboros.toml
```

**Semantics:**
- An empty `cyclic-files` list is **not** a no-op — any detected cyclic file is treated as a regression (exit 1).
- `--check-cyclic-files` does **not** fail on grandfathered cycles still listed in `cyclic-files`; it fails only when the live set diverges from the recorded list. It is **not** a replacement for `--strict`.
- Behavior is independent of `--format` (always human stderr + exit code; no JSON output).

**Known limitation:** paths are compared as display strings; on a case-insensitive filesystem (e.g. default macOS), entries differing only in case are treated as distinct — always use `--dump-cyclic-files` output verbatim to avoid case drift.

#### Including cyclic files in the JSON report (`--show-cyclic-files`)

```bash
oboros --format json --show-cyclic-files
```

Adds an optional top-level `cyclic_files` array to the JSON report. Omitted when the flag is not set (existing-consumer safe; `version` stays `1`). The HTML `report` subcommand renders a "Known Cyclic Files" section when the field is present.

#### `[cycles] ignore-derived-ancestor-init`

When `true`, files that become cyclic **only** because of a derived ancestor-`__init__.py` edge are excluded from the cyclic-files baseline. Default: `false` (existing behavior).

```toml
[cycles]
ignore-derived-ancestor-init = true
```

**Scope: baseline-only.** This option affects only `--dump-cyclic-files`, `--check-cyclic-files`, `--show-cyclic-files`, and the JSON/HTML `cyclic_files` list. The normal cycle report, `--strict`, JSON `cycles`, and HTML cycle table are unchanged.

**Mechanism:** a file is counted in the baseline only if it participates in a cycle of the *direct-import-only* graph (ancestor-`__init__.py` edges removed). Cycles that close only through a derived ancestor edge disappear from the baseline; genuine direct `__init__.py` cycles are still counted.

**Interaction with `include-ancestor-init`:** this option is a no-op when `include-ancestor-init = false` (there are no derived edges to strip). In that case the tool prints a warning to stderr.

**CLI:** `--ignore-derived-ancestor-init` forces the option on; there is no inverse flag (set `false` via config, which is the default).

**Regenerate workflow:** enabling this option changes the computed baseline. A `cyclic-files` list generated without it will be flagged as stale by `--check-cyclic-files` (the now-ancestor-only files show as `- removed`). After enabling, re-run:

```bash
oboros --dump-cyclic-files --ignore-derived-ancestor-init
```

and update `[cycles] cyclic-files`.

#### Action-flag precedence

When multiple action flags are combined, only the highest-precedence one runs:

1. `--check-cyclic-files` (highest)
2. `--dump-cyclic-files`
3. `--dump-ignores`

`--show-cyclic-files` is orthogonal and only affects the normal JSON report path.

---

## Exclude paths

The `exclude` option removes paths (files or directories) from the set of files analyzed as **seeds**, while still reporting any excluded file that is **reachable via imports from a non-excluded file**.

This mirrors [mypy's `exclude`](https://mypy.readthedocs.io/en/stable/command_line.html#cmdoption-mypy-exclude): excluded paths are dropped from recursive discovery seeds, but import-following is unaffected. Only excluded files that are unreachable from every non-excluded seed are dropped from the output.

### Configuration

```toml
# oboros.toml
exclude = ["tests", "migrations/", "legacy/old_module.py"]
```

### CLI flag

```bash
# Exclude a directory
oboros --exclude tests/

# Exclude a specific file
oboros --exclude legacy/old_module.py

# Exclude multiple paths (comma-separated or repeated)
oboros --exclude tests/,migrations/
oboros --exclude tests/ --exclude migrations/

# CLI excludes are unioned with config excludes
oboros --exclude extra_dir/
```

### Semantics

- **Excluded files that are reachable via imports from a non-excluded file are still reported.** This is the key behavior: `exclude` only removes files from the *seed* set, not from the analysis entirely.
- **Excluded files that nothing non-excluded imports are dropped.** For example, a `tests/` directory that imports app code but is not imported by app code will be dropped entirely — it is not reachable from any non-excluded seed.
- **Excluding one member of a mutual cycle with a non-excluded file does NOT hide the cycle.** Both files are mutually reachable, so both are retained.

> **Want to hide cycles inside a directory (e.g. generated code)?** `exclude` won't do that — it only trims analysis seeds, so a generated-internal cycle that your code imports is still reported. Use [`[cycles] ignore-dirs`](#cycles-ignore-dirs) instead, which drops any cycle whose files are all under the named directories.

#### Example: excluding a test directory

```toml
source-roots = ["src"]
exclude = ["tests"]
```

If `tests/test_auth.py` imports `app.auth` but nothing in `app/` imports `tests/`, then `tests/test_auth.py` is not reachable from any non-excluded seed and is dropped. Cycles within `tests/` are not reported.

If `app/auth.py` imports `tests.helpers` (unusual but possible), then `tests/helpers.py` IS reachable from the non-excluded `app/` seed and will be reported.

### Matching rules

- Patterns are matched against **project-root-relative paths** (the same paths shown in cycle output, e.g. `src/app/main.py`).
- A pattern matches either an **exact file** or a **directory prefix** (all files under that directory). A trailing `/` forces directory matching; without it, an exact file match is tried first, then directory prefix.
- A bare `app/` pattern matches across **all source roots** — there is no per-root scoping.

### Validation

- Empty-string entries (`exclude = [""]`) are rejected with an error.
- Absolute paths (`exclude = ["/abs/path.py"]`) are rejected with an error.
- `exclude = []` is valid and means "no exclusions" (no-op).
- A pattern that matches no first-party files produces a warning to stderr (exit code 0).

### Interactions

| Flag | Behavior with `--exclude` |
|------|--------------------------|
| `--trace` | Operates on the pruned graph. An excluded-unreachable path is an unknown path (warning + exit 0). |
| `--strict` | Operates on the pruned graph. `--exclude` can suppress `--strict` failures by removing cycles. |
| `--dump-ignores` | Reflects the pruned graph. Ignore entries for pruned cycles may become "unmatched" warnings. |
| `--package` | Operates on the pruned graph. |
| `[[cycles.ignore]]` | Existing ignore entries for cycles that are now pruned will produce "unmatched" warnings. |

### JSON output

When `--exclude` is used, the JSON report includes an optional top-level `excluded` array listing the applied normalized patterns (project-root-relative):

```json
{
  "version": 1,
  "summary": { "cycles_reported": 0, "cycles_suppressed": 0 },
  "cycles": [],
  "excluded": ["tests/", "migrations/"]
}
```

The `excluded` field is **omitted** when no excludes are applied (existing-consumer safe; `version` stays `1`).

### Known limitations

- **No parse-skipping speedup.** The MVP still parses all files; exclusion prunes the output graph, not the parse work. Parse-skipping is a planned future optimization.
- **Multi-source-root module-name collisions.** If two source roots both contain a file at the same relative path (e.g. `src/utils/helper.py` and `lib/utils/helper.py` both produce module `utils.helper`), the module name is ambiguous. Ouroboros now emits a warning when this is detected.
- **No per-root scoping.** A pattern like `app/` matches across all source roots.
- **`report` subcommand / HTML output** does not surface exclude information yet.

---

## Output

Ouroboros prints its results to stdout in several sections:

### Source roots

Lists each configured source root and the `.py` files discovered within it, along with their resolved module names.

```
source root: /path/to/src (42 files)
  src/pkg/__init__.py -> pkg
  src/pkg/a.py -> pkg.a
  src/pkg/b.py -> pkg.b
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

src/pkg/__init__.py
  -> src/pkg/a.py
src/pkg/a.py
  -> src/pkg/b.py
  -> src/pkg/c.py
```

### Dependency cycles

SCCs that pass the configured size filter, grouped by top-level package. Each file shows the line numbers where cycle-participating imports occur.

```
--- dependency cycles (3) ---
(1 cycles suppressed by ignore list)

package: pkg (2 cycles)

cycle 1 (3 files)
  src/pkg/a.py (imports at lines 12, 45)
  src/pkg/b.py (import at line 8)
  src/pkg/c.py (import at line 3)

cycle 2 (2 files)
  src/pkg/x.py (import at line 5)
  src/pkg/y.py (import at line 11)

(cross-package: pkg, lib) (1 cycle)

cycle 3 (2 files)
  src/pkg/foo.py (import at line 7)
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
          "path": "src/pkg/a.py",
          "import_lines": [12, 45],
          "edges": [
            { "to": "src/pkg/b.py", "lines": [12] },
            { "to": "src/pkg/c.py", "lines": [45] }
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
| `cycles[].files[].path` | string | Project-root-relative file path. |
| `cycles[].files[].import_lines` | array of integers | Sorted line numbers of imports to other cycle members. |
| `cycles[].files[].edges[].to` | string | Import target path within the cycle. |
| `cycles[].files[].edges[].lines` | array of integers | Sorted line numbers for that specific edge. |
| `cyclic_files` | array of strings | Files participating in any cycle. **Omitted** when `--show-cyclic-files` is not set. |
| `excluded` | array of strings | Applied exclude patterns. **Omitted** when no excludes were active. |

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
oboros --trace src/app/entry.py

# Short alias
oboros -t src/app/entry.py

# Trace a directory (all .py files under it)
oboros --trace src/app/

# Trace multiple paths (comma-separated or repeated flag)
oboros --trace src/app/entry.py,src/app/mid.py
oboros --trace src/app/entry.py --trace src/app/mid.py

# Combine with --format json for programmatic use
oboros --format json --trace src/app/
```

### Human output

When `--trace` is used, a `--- cycle impact ---` section is appended after the dependency cycles section:

```
--- cycle impact ---

trace: src/app/ (directory, 4 of 6 files impacted)
  src/app/core_a.py:
    impacted by 1 cycle:
      cycle 1 (member)
  src/app/core_b.py:
    impacted by 1 cycle:
      cycle 1 (member)
  src/app/entry.py:
    impacted by 1 cycle:
      cycle 1 (reachable via src/app/entry.py:1 -> src/app/mid.py:1 -> src/app/core_a.py)
  src/app/mid.py:
    impacted by 1 cycle:
      cycle 1 (reachable via src/app/mid.py:1 -> src/app/core_a.py)

trace: src/app/isolated.py (file)
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
      "path": "src/app/entry.py",
      "kind": "file",
      "files": [
        {
          "path": "src/app/entry.py",
          "impacts": [
            {
              "cycle_index": 1,
              "relationship": "reachable",
              "entry": "src/app/core_a.py",
              "from_lines": [1],
              "path": [
                { "from": "src/app/entry.py", "to": "src/app/mid.py", "lines": [1] },
                { "from": "src/app/mid.py",   "to": "src/app/core_a.py", "lines": [1] }
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
| `traced[].path` | string | The traced path as given (directory paths end with `/`). Project-root-relative. |
| `traced[].kind` | string | `"file"` or `"directory"`. |
| `traced[].files[].path` | string | Graph node path (matches `cycles[].files[].path`). Project-root-relative. |
| `traced[].files[].impacts` | array | Omitted when empty (file is clean). |
| `traced[].files[].impacts[].cycle_index` | integer | Matches `cycles[].index`. |
| `traced[].files[].impacts[].relationship` | string | `"member"` or `"reachable"`. |
| `traced[].files[].impacts[].entry` | string | First cycle member reached. Project-root-relative. |
| `traced[].files[].impacts[].from_lines` | array of integers | Import line(s) in the traced file that begin the branch. Omitted for `"member"`. |
| `traced[].files[].impacts[].path` | array of hops | Import chain from traced file to cycle entry. Omitted for `"member"`. |
| `traced[].files[].impacts[].path[].from` | string | Importing file. Project-root-relative. |
| `traced[].files[].impacts[].path[].to` | string | Imported file (next toward the cycle). Project-root-relative. |
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

## Migrating to 0.6.0

Version 0.6.0 changes all file paths from **source-root-relative** to **project-root-relative** (relative to the directory containing `oboros.toml`). For example, a file previously shown as `pkg/a.py` under `source-roots = ["src"]` now appears as `src/pkg/a.py` everywhere.

**What you need to update:**

- **`[[cycles.ignore]]` entries:** Update `files` lists to use project-root-relative paths. For example, change `files = ["pkg/a.py", "pkg/b.py"]` to `files = ["src/pkg/a.py", "src/pkg/b.py"]`. Oboros will warn if it detects pre-0.6.0 paths.
- **`[cycles] cyclic-files`:** Regenerate with `oboros --dump-cyclic-files` and update the list in your config.
- **`--trace` and `--exclude` CLI arguments:** Use project-root-relative paths. For example, `--trace src/app/entry.py` instead of `--trace app/entry.py`.
- **`oboros report --source-root`:** This flag is now `oboros report --root`. The old flag still works with a deprecation warning.
- **JSON consumers:** The schema `version` field is now `2`. All paths in `cycles[].files[].path`, `cycles[].files[].edges[].to`, `traced[].path`, `traced[].files[].path`, `traced[].files[].impacts[].entry`, `traced[].files[].impacts[].path[].from`, `traced[].files[].impacts[].path[].to`, `excluded[]`, and `cyclic_files[]` are now project-root-relative.

---



### CI gate: fail on any new cycles

```bash
oboros --strict
```

Exit code 1 if any non-suppressed cycles exist. Add `[[cycles.ignore]]` entries for known cycles to avoid false positives.

### Bootstrap an ignore list for an existing project

```bash
# Write directly into oboros.toml (recommended)
oboros --dump-ignores --write

# Or append TOML fragments (only safe when no [cycles] table exists yet)
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
# Find all cycles that affect src/app/entry.py
oboros --trace src/app/entry.py

# CI: fail only if src/app/entry.py is impacted by a cycle
oboros --trace src/app/entry.py --strict

# Trace an entire directory
oboros --trace src/app/ --format json | jq '.traced[0].files[] | select(.impacts != null)'
```

### Grandfather known cycles and block new ones

```bash
# Step 1: generate the baseline (first time or after intentional changes)
oboros --dump-cyclic-files
# Paste the output into [cycles] cyclic-files in oboros.toml

# Step 2: CI gate — fails if any new file becomes cyclic or a listed file is no longer cyclic
oboros --check-cyclic-files
```

This is complementary to `--strict`: `--strict` fails on any cycle; `--check-cyclic-files` fails only when the set of cyclic files changes from the recorded baseline.

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
