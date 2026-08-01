# repo-root-relative-paths — planning draft

slug: repo-root-relative-paths
intent: clear
review_required: false
status: plan-written-execution-started
pending_action: execute .omo/plans/repo-root-relative-paths.md via start-work (user explicitly said "start work", stepping away)
plan_file: .omo/plans/repo-root-relative-paths.md
todos: 16 (+4 final verification) — all parse for /start-work progress
classify: architecture (cross-cutting, breaking output/config contract, major version bump)

## Request (verbatim intent)
Today oboros outputs file paths stripped to each source root. User wants ALL paths in oboros
to be relative to the repo root / the directory where the root `oboros.toml` lives, instead of
source-root-stripped. Goal: eliminate confusion when multiple source roots exist and files must
be searched from the repo root. User considers this a major version bump.

## Key finding — the single origin of node identity
`crates/ouroboros-core/src/discovery/mod.rs::discover()` builds `PythonFile.rel_path` as the
**source-root-relative** walk path (e.g. `src/core/engine.py` under root `src` -> `core/engine.py`),
and derives `module_name` from that same path via `module_name_for_path`. `rel_path` is the
`PathBuf` NODE IDENTITY used by the entire graph. So changing node identity is a one-point change
at discovery: store rel_path as PROJECT-ROOT-relative (`src_root.join(walk_path)`, normalized),
while STILL deriving `module_name` from the source-root-relative walk path (module semantics MUST
NOT change — `src/core/engine.py` stays module `core.engine`, not `src.core.engine`).

"Repo root" is already well-defined: `project_root` = parent dir of resolved `oboros.toml`
(or cwd when no config found). Same anchor `discover()` already receives. No new discovery needed.

## Confirmed consumer surface (all downstream of rel_path)
- graph/build.rs: node keys + `module_to_path` (module_name->rel_path). Fixes multi-root path
  collisions (USAGE.md:361 known limitation) since full paths become unique.
- resolver/index.rs, resolve.rs: keyed on module_name only -> UNAFFECTED.
- graph/scc, condensation, edge_metadata (PathBuf,PathBuf), cycles/collect.rs: carry identity transparently.
- output.rs::package_of + cycles/filter.rs::package_of + filter_cycles_by_package: "package" = first
  path component. With project-root-relative nodes, first component becomes the SOURCE ROOT dir
  (`src`,`lib`) -> changes --package grouping, JSON `packages`, human group headers, HTML pkg table. [DECISION D1]
- output.rs::resolve_path_to_nodes / normalize_trace_path: source-root prefix stripping. Used by
  --trace AND --exclude. Path-matching semantics flip to project-root-relative. [DECISION D2]
- main.rs: --exclude, [[cycles.ignore]] files, known-cyclic-files (dump/check), --trace all become
  project-root-relative. [DECISION D2]
- report.rs::SourceLineCache::read_source_line joins each source_root+file_path to find files on disk
  (because file_path is source-root-relative today). With project-root-relative paths it must join
  project_root + file_path directly; the `report --source-root` flag effectively becomes project-root. [D2 sub]
- config.rs validation: exclude/known-cyclic-files reject absolute paths (still relative -> ok);
  doc wording about "src/ prefix stripping" changes.
- JSON schema `version` hardcoded = 1 in output.rs (JsonReport, JsonCyclicFilesReport,
  JsonDumpIgnoresReport) and checked ==1 in report.rs. Path meaning changes -> bump to 2? [DECISION D3]
- Docs README.md + USAGE.md: dozens of source-root-relative path examples + explicit statements
  (USAGE.md:321-324 "e.g. `app/main.py` not `src/app/main.py`", :83 module derivation, exclude/trace
  matching-rules sections). All need rewrite.
- Tests: unit tests in discovery/build/output/filter + integration tests in crates/ouroboros-cli/tests/*.
  16 test fixtures each with their own oboros.toml.

## Important test nuance
fixtures/generate.py writes `source-roots = ["."]`. When source root is `.`, project-root-relative
== source-root-relative, so those outputs are UNCHANGED. Behavior change is only visible for
NON-dot roots (`src`, `lib`, `packages/core`). => must ADD new tests with non-dot / multi-root
configs to cover the new behavior and the now-fixed collision case; must audit which existing
fixtures use non-dot roots (their expectations change).

## Current versions
- crates ouroboros-cli / ouroboros-core: 0.5.0 (pre-1.0). JSON schema version: 1.

## Owner-decisions (surfaced at approval gate)
- D1 package grouping: (A, rec) strip source-root prefix for package derivation so package stays the
  first dir BELOW the source root (preserves today's meaning; thread source_roots into package_of);
  (B) first component = source-root dir (simplest, but --package near-useless in multi-root).
- D2 config/CLI path surface: (A, rec) HARD BREAK — exclude / [[cycles.ignore]] / known-cyclic-files
  / --trace / --exclude all become project-root-relative; drop source-root prefix stripping; regen
  known-cyclic-files via --dump-cyclic-files; (B) backward-compat accept both (keeps ambiguity user
  wants gone).
- D3 versioning: crate 0.5.0 -> (A, rec) 0.6.0 (pre-1.0 breaking convention) or (B) 1.0.0 (signal
  stability); JSON schema version 1 -> 2 (rec yes, signals path-meaning break to consumers).
- Test strategy: rec TDD-ish tests-with-implementation per todo; agent-executed QA always included.

## Ledgers
components:
- C1 core discovery node-identity change (discovery/mod.rs) — status: planned
- C2 package grouping semantics (output.rs + cycles/filter.rs) — status: planned (D1)
- C3 path-matching + config surface (output.rs resolve_path_to_nodes, main.rs, config docs) — planned (D2)
- C4 report subcommand path resolution (report.rs) — planned
- C5 JSON/crate version bump — planned (D3)
- C6 docs (README, USAGE) — planned
- C7 tests (unit + integration + new coverage) — planned

decisions (APPROVED by user, all recommended options):
- D1 = A: strip source-root prefix for package derivation; package stays first dir BELOW source root.
- D2 = A: HARD BREAK — exclude/[[cycles.ignore]]/known-cyclic-files/--trace/--exclude become
  project-root-relative; drop source-root prefix stripping.
- D3 = A: crates 0.5.0 -> 0.6.0; JSON schema version 1 -> 2 (report.rs requires 2).
- test strategy = tests alongside implementation per todo; add new non-dot/multi-root coverage.
approval: user answered the open ambiguities at the gate => authorized to write the plan file.

## Metis gap analysis — receipt
session: ses_041b93c8ffferjODmCLPls7loo (Metis). Verdict: comprehensive findings folded into plan.
Critical: C1 main.rs read worked-example; C2 [[cycles.ignore]] silent regression -> migration guard;
C3 --check-cyclic-files silent CI break -> migration hint; C4 report SourceLineCache double-prefix
bug -> single project_root join; C5 resolve_source_roots -> project_root semantics + feed fixed cache.
High: H1/H2 make package_of source-root-aware FIRST as no-op refactor, then flip (atomic-safe);
H3/H4 traced[].path + excluded[] become project-root-relative (schema value change, doc+test);
H5 helpful warning when a --trace/--exclude input looks pre-0.6.0; H6/L1 module_to_path collision is
NOT fixed by this change (only display collision is) -> warn on module-name collision, reword limitation;
H7 fix verbose discovery output. Medium/Low: fixture matrix, overlapping-root handling (reject at
validation), root-level `.`-root files dropped by --package (doc+test), HTML package table uses strip
helper, --dump-ignores output form, version tests for all 3 structs, --source-root->--root deprecation alias.

## ALL 20 integration fixtures use source-roots = ["src"]
=> every integration-test expected path changes (pkg/a.py -> src/pkg/a.py). pyproject.toml also 0.5.0.
Adopted default (edge hardening, recorded not re-asked): REJECT overlapping/nested source roots at
config validation (prevents post-flip node-path collisions / undefined behavior).

## Critical implementation note (found during grounding)
main.rs:288 reads each file via `root.path.join(&file.rel_path)`. Once rel_path is
project-root-relative, that double-joins the source root. MUST change the on-disk read to
`project_root.join(&file.rel_path)` (project_root is in scope in main.rs). Same single-anchor
join applies to report.rs source-line reads.
