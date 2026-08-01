# repo-root-relative-paths - Work Plan

## TL;DR (For humans)

**What you'll get:** `oboros` will emit and match every file path relative to the **project root**
(the directory holding the resolved `oboros.toml`, or cwd if none) instead of stripping each source
root. `src/core/engine.py` under `source-roots = ["src"]` will show as `src/core/engine.py`
everywhere — human output, JSON (`cycles[].files[].path`, `edges[].to`, `traced[].path`, `excluded[]`,
`cyclic_files[]`), and every path you type into `--trace` / `--exclude` / `[[cycles.ignore]]` /
`[cycles] known-cyclic-files`. Dotted module names (`core.engine`) are unchanged. This also removes
the "same relative path across two source roots" ambiguity.

**Why this approach:** node identity has a single origin — `PythonFile.rel_path` built in
`discovery::discover()`. We keep deriving the module name from the source-root-relative walk path,
but store `rel_path` as project-root-relative. That one change flips identity across the whole graph,
so the rest is: make the "package" grouping source-root-aware (so `src/pkg/a.py` still groups under
`pkg`, not `src`), turn the config/CLI path surface into a clean project-root-relative contract,
fix the two on-disk read sites (`main.rs`, `report.rs`) that would otherwise double-prefix, add
migration guards so existing configs fail loudly (not silently), bump versions, and rewrite docs/tests.

**What it will NOT do:** no change to module-name derivation or import resolution; no backward-compat
that accepts old source-root-relative paths (hard break, by decision); no new output fields; no
parse-skipping / perf work; no fix to genuine *module-name* collisions across roots beyond a new
warning (that ambiguity is inherent to Python and out of scope to "resolve").

**Effort:** ~15 todos across 7 waves + a 4-check final verification wave. Bulk of the churn is
updating 20 integration-test fixtures and the docs.

**Risk:** Medium-high. It is a breaking output + config contract change (major-ish bump 0.5.0 -> 0.6.0,
JSON schema 1 -> 2). Main hazards are silent upgrade regressions in `[[cycles.ignore]]` /
`--check-cyclic-files` and double-prefix read bugs — all explicitly guarded/tested here.

**Decisions (all confirmed with you):**
- Package grouping strips the source-root prefix, so package = first dir *below* the source root (preserves today's meaning).
- Hard break: all config/CLI paths become project-root-relative; source-root prefix stripping is removed.
- Versions: crates + `pyproject.toml` 0.5.0 -> 0.6.0; JSON schema `version` 1 -> 2 (`report` requires 2).
- Tests written alongside each code change; new multi-root/edge coverage added.
- Adopted default (edge hardening): reject overlapping/nested source roots at config validation.

---

## Scope

### In scope
- `crates/ouroboros-core/src/discovery/mod.rs` — make `PythonFile.rel_path` project-root-relative; add path-normalization helper; keep module-name derivation source-root-relative.
- `crates/ouroboros-core/src/config.rs` — reject overlapping/nested source roots; doc wording.
- `crates/ouroboros-core/src/graph/build.rs` — node keys follow rel_path automatically; warn on module-name collision.
- `crates/ouroboros-core/src/cycles/filter.rs` — `package_of` / `filter_cycles_by_package` source-root-aware.
- `crates/ouroboros-cli/src/output.rs` — `package_of`/`packages_for_cycle` source-root-aware; remove source-root prefix stripping in `resolve_path_to_nodes`/`normalize_trace_path`; helpful no-match warning.
- `crates/ouroboros-cli/src/main.rs` — fix on-disk read to `project_root.join`; verbose output; thread `source_roots` into package grouping; migration guards for `[[cycles.ignore]]` and `--check-cyclic-files`.
- `crates/ouroboros-cli/src/report.rs` — `SourceLineCache` single `project_root` join; `resolve_source_roots` -> project root; rename `report --source-root` to `--root` (deprecated alias kept).
- Version bump: `crates/ouroboros-core/Cargo.toml`, `crates/ouroboros-cli/Cargo.toml`, `pyproject.toml`; JSON schema `version` in the 3 output structs + `report::load_json_report`.
- Docs: `README.md`, `USAGE.md` (incl. a 0.6.0 migration section).
- Tests: all unit tests touching paths; all 20 `crates/ouroboros-cli/tests/fixtures/*` + integration `tests/*.rs`; new multi-root/edge fixtures.

### Out of scope (Must-NOT-Have)
- No change to `module_name_for_path`, `ModuleIndex`, or `resolver/*` resolution logic (module names stay source-root-relative).
- No backward-compatible acceptance of old source-root-relative config/CLI paths.
- No new JSON fields; no change to field *names* (only path *values* and `version`).
- No resolution of genuine cross-root module-name collisions beyond emitting a warning.
- No performance/parse-skipping work; no HTML redesign beyond correct path rendering.
- No auto-migration/rewriting of users' `oboros.toml`.

### Anchor definitions
- **project_root** = `config_path.parent()` for the resolved `oboros.toml`, else cwd (already computed in `main.rs:210-229`). This is "repo root / where the root oboros.toml lives".
- **node identity** = `PythonFile.rel_path` (a `PathBuf`), now project-root-relative.
- **module name** = derived from the *source-root-relative* walk path (unchanged).

---

## Verification strategy
- Test approach: tests written alongside each todo (unit tests in the touched module; integration
  tests in `crates/ouroboros-cli/tests/`). Existing expectations that change purely due to path form
  are updated in the SAME wave that flips them, so `cargo test` is green after each wave's commit.
- Per-todo agent-executed QA: every todo lists a happy-path and a failure-path scenario with the exact
  command and an evidence artifact path under `/tmp/oboros-qa/`.
- Global gates (run in the final wave and after each code wave): `cargo build --workspace`,
  `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`.
- Real end-to-end QA: build the binary and run it against a purpose-built multi-source-root sample
  project (created under `/tmp/oboros-qa/multiroot/`) to confirm full paths, package grouping, trace,
  exclude, ignore, known-cyclic-files, and `report` HTML all behave.

---

## Execution strategy
- Ordering is dependency-driven and atomicity-safe:
  - **Wave 1** makes package grouping source-root-aware as a **no-op refactor on today's
    (source-root-relative) paths** — nothing to strip yet, so all tests stay green. This removes the
    W1/W2 atomicity hazard Metis flagged (`--package` never sees a bare `src` component).
  - **Wave 2** flips node identity and updates every existing unit + integration expectation that
    changes purely due to path form, so `cargo test` is green immediately after the flip commit.
  - **Waves 3-5** are the config-surface hard break, the `report` subcommand, and version bumps,
    each self-contained with its own tests + migration guards.
  - **Waves 6-7** are docs and new/edge-case coverage.
- Each todo is a single atomic commit (see Commit strategy). A wave may be >1 commit.
- Subagents may be used for the mechanical fixture-expectation sweep (Wave 7.1) and doc rewrite
  (Wave 6), but each change must be verified by the executor running the gates.

---

## Todos

### Wave 1 — Make package grouping source-root-aware (behavior-preserving refactor)

- [x] 1. **(W1) `ouroboros-core`: add `strip_source_root_prefix` helper + unit tests.**
  - References: new helper in `crates/ouroboros-core/src/graph/mod.rs` (or a new `path_util` module re-exported from `graph`); consumed later by `output.rs` and `cycles/filter.rs`. Model normalization on `output.rs::normalize_trace_path:412-416` (strip leading `./`, trailing `/`, `\\`->`/`).
  - Behavior: `strip_source_root_prefix(path: &Path, source_roots: &[String]) -> &Path` returns the longest matching `source_root/`-prefixed remainder, else the original path. Normalizes each source root (trim trailing `/`, `.`/"" means no prefix). On empty `source_roots`, returns the path unchanged.
  - Acceptance: `cargo test -p ouroboros-core` green; unit tests cover: `("src/pkg/a.py", ["src"]) -> "pkg/a.py"`; `("pkg/a.py", []) -> "pkg/a.py"`; `("src/a.py", ["src"]) -> "a.py"`; `("lib/x.py", ["src","lib"]) -> "x.py"`; nested-root longest-match `("src/pkg/a.py", ["src","src/pkg"]) -> "a.py"`; no-match `("app/a.py", ["src"]) -> "app/a.py"`.
  - QA happy: `cargo test -p ouroboros-core strip_source_root_prefix -- --nocapture > /tmp/oboros-qa/w1_helper.txt` shows all cases pass.
  - QA failure: temporarily assert a wrong expectation locally to confirm the test fails (revert) — evidence `/tmp/oboros-qa/w1_helper_negative.txt`.
  - Commit: `feat(core): add source-root prefix strip helper for package grouping`.

- [x] 2. **(W1) Make `package_of`/`packages_for_cycle` (output.rs) and `package_of`/`filter_cycles_by_package` (filter.rs) source-root-aware; thread `source_roots` through call sites.**
  - References: `crates/ouroboros-cli/src/output.rs:142-181` (`package_of`, `packages_for_cycle`, `order_cycles`), `:183-247` (`build_json_report`), `crates/ouroboros-cli/src/main.rs:568-623` (human grouping), `:452-456` + `:709-724` (call sites), `crates/ouroboros-core/src/cycles/filter.rs:59-81` (`package_of`, `filter_cycles_by_package`) and its `main.rs:452-453` call site.
  - Behavior: each `package_of` first applies `strip_source_root_prefix` (1.1), then takes the first component. Thread `&config.source_roots` (or `&[String]`) into `packages_for_cycle`, `order_cycles`, `build_json_report`, the human grouping loop, and `filter_cycles_by_package`. On today's source-root-relative paths with real roots, stripping is a **no-op** (paths have no `src/` prefix yet) — existing outputs unchanged.
  - Acceptance: `cargo test --workspace` green with NO expectation changes (proves no-op). Update the unit-test call sites in `output.rs` tests (`packages_for_cycle(&cycle)` etc.) and `filter.rs` tests (`filter_cycles_by_package(cycles)`) to pass source_roots; use `&[]` where inputs are bare (preserves current expected packages).
  - QA happy: `cargo test --workspace > /tmp/oboros-qa/w1_2.txt` all green; add a new unit test proving future behavior: `packages_for_cycle(["src/pkg/a.py"], ["src"]) == ["pkg"]`.
  - QA failure: unit test `packages_for_cycle(["src/pkg/a.py"], [])` must yield `["src"]` (no strip) — asserts the helper is actually applied only when roots are given.
  - Commit: `refactor(cli): make package grouping source-root-aware (no-op on current paths)`.

### Wave 2 — Flip node identity to project-root-relative

- [x] 3. **(W2) `discovery::discover()`: store `rel_path` project-root-relative; keep module name source-root-relative; add normalization; update `PythonFile` doc + unit tests.**
  - References: `crates/ouroboros-core/src/discovery/mod.rs:14-77` (`PythonFile`, `discover`), `discovery/module_name.rs:15` (`module_name_for_path`), `discovery/walk.rs:8-24` (returns source-root-relative walk paths).
  - Behavior: for each `src_root` and each `walk_rel`: `module_name = module_name_for_path(&walk_rel)` (unchanged); `rel_path = node_path(src_root, &walk_rel)` where `node_path` normalizes: if `src_root` trimmed is `""`/`"."` -> `walk_rel`; else `PathBuf::from(src_root.trim_end_matches('/')).join(&walk_rel)`. Update the `PythonFile.rel_path` doc to "Path relative to the project root (e.g. `src/core/engine.py`)."
  - Acceptance: `cargo test -p ouroboros-core` green. Update `discover_single_root` to expect `["src/app.py","src/core/__init__.py","src/core/engine.py"]` while module names stay `["app","core","core.engine"]`; `discover_dot_root` stays `app.py`,`models/user.py`; `discover_multiple_roots` -> `src/a.py`,`lib/b.py`. Add a normalization unit-test table: root `"."`,`"src"`,`"src/"`,`"src\\pkg"` for file `a/b.py`.
  - QA happy: `cargo test -p ouroboros-core discovery -- --nocapture > /tmp/oboros-qa/w2_1.txt`.
  - QA failure: assert module name is NOT `src.core.engine` (guard against accidentally deriving module from the prefixed path) — evidence in same file.
  - Commit: `feat(core)!: node paths are project-root-relative (module names unchanged)`.

- [x] 4. **(W2) `main.rs`: fix on-disk read + verbose discovery output for the new identity.**
  - References: `crates/ouroboros-cli/src/main.rs:286-296` (read loop `root.path.join(&file.rel_path)`), `:255-269` (verbose discovery print).
  - Behavior: change the read to `let abs_path = project_root.join(&file.rel_path);` — worked example: `project_root = /proj`, `rel_path = src/core/engine.py` -> `/proj/src/core/engine.py` (correct); the old `root.path.join` would give `/proj/src/src/core/engine.py`. Verbose output: print `project root: {project_root}` once, then per file `  {rel_path} -> {module_name}` (now project-root-relative); keep the per-root header but do not double-show the prefix confusingly (show root count only, or print `source root: {root.path}` then project-root-relative files — pick the form that reads cleanly and document it in the commit).
  - Acceptance: `cargo run -p ouroboros-cli -- --config <multiroot>/oboros.toml -v` reads all files without "could not read" warnings; verbose lines show `src/...`/`lib/...` paths.
  - QA happy: build multiroot sample (see 7.2), run `oboros -v`, capture `/tmp/oboros-qa/w2_2.txt` — no read warnings, project-root-relative file list.
  - QA failure: point at a project whose file is missing and confirm the read-warning path still fires with the correct absolute path.
  - Commit: `fix(cli): read source files via project_root and show project-root-relative verbose output`.

- [x] 5. **(W2) `graph/build.rs`: warn on module-name collision across roots (identity now unique).**
  - References: `crates/ouroboros-core/src/graph/build.rs:22-61` (`module_to_path` last-writer-wins).
  - Behavior: when inserting into `module_to_path`, if the module name is already present with a *different* `rel_path`, collect it and emit a single stderr warning from the CLI (return collision info from core, or log via a returned `Vec<(String, Vec<PathBuf>)>` surfaced in `main.rs`). Keep resolution behavior (last-writer-wins) — this only informs the user of a genuine `utils.helper` vs `utils.helper` ambiguity. Node keys become project-root-relative automatically (no code change to keys).
  - Acceptance: `cargo test -p ouroboros-core` green; unit test: two roots producing the same module name yields one collision record; distinct module names yield none.
  - QA happy: multiroot sample with `src/utils/helper.py` + `lib/utils/helper.py` prints exactly one collision warning; capture `/tmp/oboros-qa/w2_3.txt`.
  - QA failure: single-root project prints no collision warning.
  - Commit: `feat(core): warn on cross-root module-name collisions`.

- [x] 6. **(W2) Sweep existing unit + integration expectations that change purely due to path form (keep suite green after the flip).**
  - References: unit tests in `graph/build.rs`, `output.rs`, `cycles/filter.rs`, `cycles/collect.rs` that assert bare paths remain valid (they use fabricated paths, not discovery — audit each; most need NO change). Integration: `crates/ouroboros-cli/tests/*.rs` (`format_json.rs`, `exclude.rs`, `trace.rs`, `cyclic_files.rs`, `report.rs`, `package_filter.rs`, `ancestor_init.rs`, `package_relative_init.rs`, `ignore_derived_ancestor_init.rs`) with fixtures under `tests/fixtures/*` (all use `source-roots = ["src"]`).
  - Behavior: update expected paths from source-root-relative (`app/a.py`) to project-root-relative (`src/app/a.py`) for every fixture that uses a non-dot root. Do NOT change `--trace`/`--exclude`/ignore/known input strings yet if a test would then fail — those input-form changes belong to Wave 3; if a test couples both, move its update into the wave that makes it pass and leave a `// updated in W3` note. Package-grouping assertions must still show the package below the source root (e.g. `src/pkg/a.py` -> package `pkg`) thanks to Wave 1.
  - Acceptance: `cargo test --workspace` fully green.
  - QA happy: `cargo test --workspace > /tmp/oboros-qa/w2_4.txt` — 0 failures.
  - QA failure: `cargo test --workspace 2>&1 | grep -c FAILED` returns 0 (evidence appended).
  - Commit: `test!: update expectations to project-root-relative paths`.

### Wave 3 — Config/CLI path surface hard break (project-root-relative)

- [ ] 7. **(W3) Remove source-root prefix stripping from `--trace`/`--exclude`; add a helpful no-match hint.**
  - References: `crates/ouroboros-cli/src/output.rs:381-416` (`resolve_path_to_nodes`, `normalize_trace_path`), `:249-379` (`build_traces` passes `source_roots`), `main.rs:369-385` (exclude loop), `:625-635` + `:697-707` (trace calls).
  - Behavior: match the normalized input directly against the (now project-root-relative) node set — delete the `source_roots` fallback branch. Drop the `source_roots` parameter from `resolve_path_to_nodes`/`build_traces` (or keep only for the hint). On no match, if prepending any `source_root/` to the input WOULD have matched a node, emit: `warning: '<input>' matched no files; paths are project-root-relative in 0.6.0 — did you mean '<src>/<input>'?`. `JsonTrace.path`/`excluded[]` display forms are now project-root-relative (H3/H4) — no special handling, just verify.
  - Acceptance: `cargo test --workspace` green after test updates; integration tests use project-root-relative `--trace src/app/...` / `--exclude src/tests/...`.
  - QA happy: on multiroot sample, `oboros --trace src/app/entry.py` and `oboros --exclude src/tests/` behave; capture `/tmp/oboros-qa/w3_1.txt`.
  - QA failure: `oboros --trace app/entry.py` (old form) prints the "did you mean 'src/app/entry.py'?" hint and exits 0 with no match; capture evidence.
  - Commit: `feat(cli)!: --trace/--exclude paths are project-root-relative`.

- [ ] 8. **(W3) Migration guard for `[[cycles.ignore]]` mismatches.**
  - References: `crates/ouroboros-cli/src/main.rs:432-450` (ignore-entry warning loop), `cycles/filter.rs:35-57` (`filter_ignored_cycles` exact-set compare).
  - Behavior: matching logic unchanged (both sides project-root-relative now). Enhance the "did not match" warning: if the unmatched entry's files, once each is prefixed by some `source_root/`, WOULD match a detected cycle, emit a targeted hint: `ignore entry looks pre-0.6.0 (source-root-relative); rewrite to project-root-relative, e.g. 'src/pkg/a.py'`.
  - Acceptance: `cargo test --workspace` green; new integration test `ignore_migration_hint` with a `["src"]` fixture whose ignore entry omits the `src/` prefix triggers the hint and does NOT suppress the cycle.
  - QA happy: run the fixture, capture the hint to `/tmp/oboros-qa/w3_2.txt`.
  - QA failure: a correctly project-root-relative ignore entry suppresses the cycle and emits no hint.
  - Commit: `feat(cli): migration hint for pre-0.6.0 [[cycles.ignore]] paths`.

- [ ] 9. **(W3) Migration guard for `--check-cyclic-files`; confirm `--dump-cyclic-files`/`--dump-ignores` emit project-root-relative.**
  - References: `crates/ouroboros-cli/src/main.rs:461-535` (`--check-cyclic-files`, `--dump-cyclic-files`), `:536-556` (`--dump-ignores`), `output.rs:102-114` + `:444-458` (dump report builders).
  - Behavior: on `--check-cyclic-files` mismatch, if every "added" path equals some `source_root/` + a "removed" path, emit before exiting 1: `known-cyclic-files uses pre-0.6.0 source-root-relative paths; run 'oboros --dump-cyclic-files' to regenerate.` `--dump-cyclic-files`/`--dump-ignores` already print node paths, so they now emit project-root-relative automatically — add assertions.
  - Acceptance: `cargo test --workspace` green; integration tests: `check_cyclic_files_migration_hint` (stale source-root-relative baseline -> hint + exit 1); `dump_cyclic_files_project_root_relative` (asserts `src/...`).
  - QA happy: run both on the `cyclic_*` fixtures, capture `/tmp/oboros-qa/w3_3.txt`.
  - QA failure: an up-to-date project-root-relative baseline prints `cyclic files unchanged` and exits 0.
  - Commit: `feat(cli): migration hint for pre-0.6.0 known-cyclic-files; dumps emit project-root-relative`.

### Wave 4 — `report` subcommand path resolution

- [ ] 10. **(W4) Fix `SourceLineCache` + `resolve_source_roots` to a single project_root; rename `--source-root` to `--root` (deprecated alias); verify HTML rendering.**
  - References: `crates/ouroboros-cli/src/report.rs:100-135` (`resolve_source_roots`), `:137-190` (`SourceLineCache`), `:491-588` (`write_cycle_table` uses `file.path`), `:453-471`+`:16-53` (`ReportStats` package frequency), `crates/ouroboros-cli/src/main.rs:26-38` (`Commands::Report` args), `:188-196` (dispatch).
  - Behavior: `SourceLineCache` holds a single `project_root: PathBuf` and reads `project_root.join(file_path)` (fixes the double-prefix bug — with project-root-relative `file.path` this is exact). Replace `resolve_source_roots` with `resolve_project_root` (explicit flag value, else discover via `find_config` -> parent). Rename the `Report { source_root }` arg to `root: Option<PathBuf>`; accept `--source-root` as a hidden deprecated alias that warns and maps to `root`. `ReportStats::from_report` package grouping must apply the Wave-1 strip helper (but the report has no `source_roots`; since JSON `packages` is already computed correctly by the producer, prefer using `cycle.packages` directly rather than re-deriving from `file.path`) — verify `write_package_table` uses `cycle.packages`, not a re-split of `file.path`.
  - Acceptance: `cargo test --workspace` green; new `report.rs` test: given a project-root-relative JSON + a real project root, source-line annotations are non-empty; and a test that `--source-root` still works with a deprecation warning.
  - QA happy: `oboros --format json > r.json && oboros report --root <multiroot> r.json -o /tmp/oboros-qa/report.html` — open/grep the HTML for a real import line and correct `src/...` paths; evidence `/tmp/oboros-qa/w4_1.txt`.
  - QA failure: `oboros report r.json -o out.html` with NO root still renders (paths shown, source lines gracefully omitted, no crash/double-prefix).
  - Commit: `fix(cli)!: report reads source lines via project root; rename --source-root to --root`.

### Wave 5 — Version bump

- [ ] 11. **(W5) Bump crate + wheel versions to 0.6.0; JSON schema `version` 1 -> 2; require 2 on load.**
  - References: `crates/ouroboros-core/Cargo.toml:3`, `crates/ouroboros-cli/Cargo.toml:3`, `pyproject.toml:7`, `output.rs:107` + `:236` + `:455` (`version: 1` in `JsonCyclicFilesReport`, `JsonReport`, `JsonDumpIgnoresReport`), `report.rs:60-65` (`load_json_report` checks `version != 1`).
  - Behavior: set all three crate/wheel versions to `0.6.0`; set the three struct `version` fields to `2`; change `load_json_report` to require `version == 2` with a clear error naming the 0.6.0 break (`unsupported report version: {v} (expected 2; regenerate with oboros 0.6.0)`).
  - Acceptance: `cargo build --workspace` green; unit tests asserting each of the 3 serialized reports has `version == 2`; a `report.rs` test that a `version:1` JSON is rejected with the migration error.
  - QA happy: `oboros --format json | jq .version` prints `2`; `oboros --dump-cyclic-files --format json | jq .version` prints `2`; evidence `/tmp/oboros-qa/w5_1.txt`.
  - QA failure: feed a hand-written `version:1` report to `oboros report` -> clear rejection, exit non-zero.
  - Commit: `chore!: bump to 0.6.0 and JSON schema version 2`.

### Wave 6 — Documentation

- [ ] 12. **(W6) Rewrite `USAGE.md` for project-root-relative paths + add a 0.6.0 migration section.**
  - References: `USAGE.md` — `:23` (`--trace` "relative to a source root"), `:47` (config paths), `:83` (module derivation note — keep, it's about module names), exclude matching rules `:319-324` (flip "e.g. `app/main.py` not `src/app/main.py`"), `:349-356` (`excluded` field), trace section examples `:534-551`, JSON field table `:498-503` + `:623-637` (`path`, `traced[].path` now project-root-relative), known-cyclic-files `:168-258`, multi-source-root collision limitation `:358-363`, output examples `:375-455`.
  - Behavior: update every path example to project-root-relative (`src/pkg/a.py`); flip the matching-rule statements; split the collision limitation — the display/exclude collision is **fixed**, keep a reworded note that cross-root **module-name** collisions are ambiguous and now emit a warning; add a top-level "Migrating to 0.6.0" section: config paths, `[[cycles.ignore]]`, `known-cyclic-files` (regenerate via `--dump-cyclic-files`), `--trace`/`--exclude`, `report --root`, JSON `version` 2.
  - Acceptance: no source-root-relative path examples remain except where illustrating module names; `grep -n "not \`src/" USAGE.md` returns nothing; migration section present.
  - QA happy: manual read-through diff; `grep -n "src/" USAGE.md > /tmp/oboros-qa/w6_1.txt` shows consistent usage.
  - QA failure: `grep -nE "app/main.py.*not.*src" USAGE.md` returns nothing (old statement gone).
  - Commit: `docs: USAGE for project-root-relative paths + 0.6.0 migration`.

- [ ] 13. **(W6) Update `README.md`.**
  - References: `README.md:52-76` (quick start / flags), `:99-107` (how-it-works phase 1 example — module name mapping stays), feature bullets `:11-25`.
  - Behavior: adjust any path-form references; note that output/config paths are project-root-relative as of 0.6.0; leave module-name examples intact.
  - Acceptance: README consistent with USAGE; no stale "stripped to source root" phrasing.
  - QA happy: read-through; `grep -n "source root" README.md > /tmp/oboros-qa/w6_2.txt` reviewed.
  - QA failure: n/a (doc) — reviewer confirms no contradiction with USAGE migration section.
  - Commit: `docs: README for project-root-relative paths`.

### Wave 7 — New & edge-case test coverage

- [ ] 14. **(W7) Enumerate + finish the 20-fixture expectation sweep (checklist).**
  - References: all `crates/ouroboros-cli/tests/fixtures/*/oboros.toml` (20, all `["src"]`) and their integration tests. Build a before/after table (e.g. `cyclic_basic`: `app/a.py` -> `src/app/a.py`) as review evidence.
  - Behavior: confirm every fixture-derived assertion updated in Waves 2-3 is consistent; fill any missed ones; commit the checklist as a comment block in the relevant test or in `/tmp/oboros-qa/fixture_matrix.md`.
  - Acceptance: `cargo test --workspace` green; checklist covers all 20.
  - QA happy: `cargo test -p ouroboros-cli > /tmp/oboros-qa/w7_1.txt` — 0 failures.
  - QA failure: `grep -rn '"app/' crates/ouroboros-cli/tests` returns only intentional cases (dot-root or input-hint tests).
  - Commit: `test: complete project-root-relative fixture sweep`.

- [ ] 15. **(W7) Add multi-root + collision + `--package` + trace/exclude coverage.**
  - References: new fixture `tests/fixtures/multiroot/` with `source-roots = ["src","lib"]`, including `src/utils/helper.py` and `lib/utils/helper.py` (same module name, distinct nodes) and a cross-root cycle; new integration test module.
  - Behavior: assert (a) output shows `src/...` and `lib/...` full paths; (b) the two `utils/helper.py` files are distinct nodes and both appear; (c) a cross-root module-name collision warning is emitted; (d) `--package` groups `src/pkg/*` under `pkg` (not `src`); (e) `--trace src/app/x.py` and `--exclude lib/legacy/` work; (f) `[[cycles.ignore]]` + `known-cyclic-files` with project-root-relative paths suppress/match correctly.
  - Acceptance: new tests pass; `cargo test --workspace` green.
  - QA happy: `cargo test -p ouroboros-cli multiroot > /tmp/oboros-qa/w7_2.txt`.
  - QA failure: assert that `--package` on this fixture does NOT produce a package named `src` or `lib`.
  - Commit: `test: multi-source-root, collision, package/trace/exclude coverage`.

- [ ] 16. **(W7) Edge-case coverage: dot-root mixed with non-dot; overlapping-root rejection; `.`-root files dropped by `--package`.**
  - References: `crates/ouroboros-core/src/config.rs:148-228` (`validate`) for overlapping-root rejection; new fixtures `tests/fixtures/mixed_dot_root/` (`[".","lib"]`) and a validation unit test for `["src","src/pkg"]`.
  - Behavior: (a) `[".","lib"]` -> dot-root files show unprefixed (`app.py`), lib files show `lib/...`, and `--package` groups lib files by `lib` (documented); (b) config validation REJECTS overlapping/nested source roots (`["src","src/pkg"]`, or `["src","src"]`) with a clear `ConfigError::Validation`; (c) with `source-roots = ["."]`, root-level files (`app.py`) have no package and are dropped by `--package` — explicit test + a one-line note in USAGE.
  - Acceptance: `cargo test --workspace` green; validation unit tests for overlap/nesting; integration test for mixed dot root.
  - QA happy: `cargo test overlap dot_root -- --nocapture > /tmp/oboros-qa/w7_3.txt`.
  - QA failure: `["src","src/pkg"]` config fails to load with the overlap error message.
  - Commit: `feat(core): reject overlapping source roots; test dot-root & --package edges`.

---

## Final verification wave (all must pass; runs in parallel, then wait for user)
- **F1 — Plan compliance audit:** every todo landed with references + tests + QA evidence under
  `/tmp/oboros-qa/`; `[[cycles.ignore]]`, `--check-cyclic-files`, `report` double-prefix, and
  `traced[]`/`excluded[]` schema-value changes are all covered.
- **F2 — Code quality:** `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo fmt --check` clean; no dead `source_roots` params left after stripping removal; collision
  warning path reviewed.
- **F3 — Real manual QA:** build `oboros`; run against `/tmp/oboros-qa/multiroot/` for human + JSON +
  `--trace` + `--exclude` + `--package` + `--dump-cyclic-files` + `--check-cyclic-files` + `report --root`;
  confirm full project-root-relative paths and the migration hints on a deliberately-stale config.
- **F4 — Scope fidelity:** module names unchanged (spot-check `-v` output); no backward-compat path
  acceptance; no new JSON fields; versions at 0.6.0 / schema 2.

---

## Commit strategy
- One atomic commit per todo, messages as listed (Conventional Commits; `!` marks the breaking ones:
  2.1, 2.4, 3.1, 4.1, 5.1). Wave 1 lands before Wave 2 so `--package` is correct at every commit.
- After each code wave: run `cargo build/test/clippy/fmt` and only commit when green.
- Do not squash the breaking commits into unrelated changes; the 0.6.0 bump (5.1) is its own commit.
- Keep unrelated working-tree changes out of scope; if the tree is dirty at start, record and exclude.

## Success criteria
- `src/core/engine.py` (under `source-roots=["src"]`) appears as `src/core/engine.py` in human output,
  JSON `path`/`edges.to`/`traced[].path`/`excluded[]`/`cyclic_files[]`; dotted module name stays `core.engine`.
- `--trace`, `--exclude`, `[[cycles.ignore]]`, `known-cyclic-files` all accept/emit project-root-relative
  paths; pre-0.6.0 source-root-relative usage produces a loud, specific migration hint (never silent).
- Multi-root repos: `src/utils/helper.py` and `lib/utils/helper.py` are distinct; `--package` still
  groups by the package below the source root; overlapping source roots are rejected at load.
- `report --root` renders correct source lines (no double-prefix); `--source-root` still works with a
  deprecation warning.
- Versions: crates + wheel `0.6.0`; JSON schema `version: 2`; `oboros report` rejects `version: 1`.
- `cargo build/test/clippy/fmt` all clean; docs (README + USAGE) consistent with a 0.6.0 migration section.
