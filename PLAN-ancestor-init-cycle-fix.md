# Fix Plan: `is_ancestor_or_self` guard suppresses real cycles

## Problem

`push_ancestor_package_deps()` in `crates/ouroboros-core/src/resolver/resolve.rs:104` skips ancestor `__init__.py` edges when the source module lives inside the same package tree. This suppresses real cycles where `P/__init__.py → P.child → P/__init__.py`.

### Reproduction

```
P/__init__.py:  from P.child import X
P/child.py:    from . import sibling
```

Oboros resolves `P/child.py → P/sibling.py` (direct) and skips the ancestor edge `P/child.py → P/__init__.py` because `P` is an ancestor of `P.child`. The cycle `P/__init__.py → P/child.py → P/__init__.py` is invisible.

### Affected real-world cycles

- `eta_batch_v2/__init__.py → eta_batch_main.py → eta_batch_v2/__init__.py` (`from . import redis_store_v2`)
- `ai_automation/db/__init__.py → models.py → ai_automation/db/__init__.py` (`from ...db import constants`)
- `bulk_offboarding/__init__.py → field_definitions.py → bulk_offboarding/__init__.py` (`from ...bulk_offboarding import fields`)
- `bulk_offboarding/__init__.py → fields.py → bulk_offboarding/__init__.py` (`from ...bulk_offboarding import constants`)

## Root Cause

`resolve.rs:101-104`:
```rust
// Skip prefixes that are the source module or one of its ancestor
// packages — those are already initialized on the source's own import
// path, so edging back to them fabricates self-tree cycles.
if !is_ancestor_or_self(&prefix, source_module) && index.contains(&prefix) {
    deps.push(ResolvedDep { ... });
}
```

The guard assumes that if `P.child` lives inside `P`, then `P/__init__.py` is already initialized when `P.child` runs, so `P.child → P` is not a new dependency. This is **wrong when `P/__init__.py` is the one that imported `P.child`** — then `P` is still being initialized, and the edge back to `P` creates a real cycle.

## Fix: Two-pass approach

The fix cannot be done inline in `push_ancestor_package_deps` because it doesn't have access to the full import graph — it only sees one file's imports at a time. The fix requires a post-resolution pass.

### Step 1: Record suppressed edges during resolution

**File:** `crates/ouroboros-core/src/resolver/resolve.rs`

Modify `push_ancestor_package_deps` to return a list of suppressed `(source, ancestor_package, line)` triples instead of silently dropping them.

Add a new struct:
```rust
/// An ancestor-init edge that was suppressed by the is_ancestor_or_self guard
/// and may need to be restored if it participates in a cycle.
pub struct SuppressedAncestorEdge {
    pub source: String,
    pub ancestor_package: String,
    pub line: u32,
}
```

Modify `FileResolution` to include `suppressed_ancestor_edges: Vec<SuppressedAncestorEdge>`.

Modify `resolve_file_imports` to collect suppressed edges from `push_ancestor_package_deps` and return them in `FileResolution`.

Modify `ResolveResult` to include `suppressed_ancestor_edges: Vec<SuppressedAncestorEdge>`.

Modify `resolve_all` in `mod.rs` to aggregate suppressed edges across all files.

### Step 2: Restore edges that participate in cycles

**File:** `crates/ouroboros-core/src/graph/build.rs` (or a new `graph/restore.rs`)

After building the initial file dependency graph (which excludes suppressed edges), add a restoration pass:

```rust
/// Restore suppressed ancestor-init edges that participate in cycles.
///
/// A suppressed edge `source → ancestor_package` is restored if and only if
/// `ancestor_package` already has a path to `source` in the graph (i.e., the
/// ancestor `__init__.py` imports the source module, directly or indirectly).
/// In that case, the suppressed edge closes a real cycle and must be present
/// for SCC detection to find it.
///
/// Edges where no path exists from `ancestor_package` back to `source` are
/// left suppressed — they are false positives that would create noise.
pub fn restore_cyclic_ancestor_edges(
    graph: &mut FileDependencyGraph,
    edge_metadata: &mut EdgeMetadata,
    module_to_path: &HashMap<&str, &PathBuf>,
    suppressed: &[SuppressedAncestorEdge],
) {
    for edge in suppressed {
        let from_path = module_to_path.get(edge.source.as_str());
        let to_path = module_to_path.get(edge.ancestor_package.as_str());
        if let (Some(from), Some(to)) = (from_path, to_path) {
            // Check if `ancestor_package` can reach `source` in the current graph.
            // If yes, this suppressed edge closes a real cycle — restore it.
            if has_path(graph, to, from) {
                graph.entry(from.clone()).or_default().insert(to.clone());
                edge_metadata
                    .entry((from.clone(), to.clone()))
                    .or_default()
                    .push(edge.line);
            }
        }
    }
}
```

`has_path` is a simple BFS/DFS reachability check from `to` to `from` in the graph. Since this is only called for suppressed edges (typically a small number), the performance impact is negligible.

### Step 3: Wire into the pipeline

**File:** `crates/ouroboros-core/src/lib.rs` (or wherever the pipeline is orchestrated)

After `build_file_dependency_graph` and before SCC computation, call:
```rust
restore_cyclic_ancestor_edges(
    &mut graph_result.graph,
    &mut graph_result.edge_metadata,
    &module_to_path,
    &resolve_result.suppressed_ancestor_edges,
);
```

The `module_to_path` map is already built inside `build_file_dependency_graph` — it needs to be returned or made accessible. Consider returning it as part of `FileGraphResult` or restructuring slightly.

### Step 4: Add tests

**File:** `crates/ouroboros-core/src/resolver/resolve.rs` (or `graph/build.rs` tests)

Add test cases:

1. **Real cycle restored:** `P/__init__.py` imports `P.child`, `P.child` does `from . import sibling`. The suppressed edge `P.child → P` should be restored, and the SCC should include `P/__init__.py` and `P.child`.

2. **False positive stays suppressed:** `P.child` imports `P.sibling` but `P/__init__.py` does NOT import `P.child`. The suppressed edge `P.child → P` should NOT be restored (no cycle).

3. **Indirect cycle restored:** `P/__init__.py` imports `P.a`, `P.a` imports `P.b`, `P.b` does `from . import c`. The suppressed edge `P.b → P` should be restored because `P → P.a → P.b` is a path from `P` to `P.b`.

4. **Deep ancestor restored:** `P.sub/__init__.py` imports `P.sub.child`, `P.sub.child` imports `P.other` (cross-tree). The ancestor edge `P.sub.child → P` is NOT suppressed (cross-tree), but `P.sub.child → P.sub` IS suppressed. If `P.sub` has a path to `P.sub.child`, restore it.

5. **Existing tests still pass:** All existing tests in `resolve.rs` and `build.rs` should continue to pass without modification.

### Step 5: Add a fixture for integration testing

**File:** `fixtures/` (new directory)

Create a minimal project that reproduces the `eta_batch_main.py` pattern:
```
fixtures/ancestor_init_cycle/
  pkg/
    __init__.py      # from pkg.child import X
    child.py         # from . import sibling
    sibling.py       # (empty or minimal)
  oboros.toml
```

Add an integration test that runs the full oboros pipeline on this fixture and asserts the SCC includes `pkg/__init__.py` and `pkg/child.py`.

## Performance

The `has_path` check is O(V+E) per suppressed edge in the worst case. In practice:
- The number of suppressed edges is small (proportional to the number of same-tree submodule imports)
- Most `has_path` checks will be fast (early termination when the path is found)
- For a large codebase, consider caching reachability with a transitive closure if profiling shows this is a bottleneck

## Alternative considered and rejected

**Always add the direct parent edge (don't skip it):** This would surface all real cycles but also produce false positives for every `P.child → P.sibling` import where `P/__init__.py` doesn't import `P.child`. The two-pass approach is more precise — it only restores edges that actually close cycles.

## Files to modify

| File | Change |
|---|---|
| `crates/ouroboros-core/src/resolver/resolve.rs` | Return suppressed edges from `push_ancestor_package_deps`; add `SuppressedAncestorEdge` struct |
| `crates/ouroboros-core/src/resolver/mod.rs` | Add `SuppressedAncestorEdge` to `FileResolution` and `ResolveResult`; aggregate in `resolve_all` |
| `crates/ouroboros-core/src/graph/build.rs` | Return `module_to_path` from `FileGraphResult` (or make it accessible) |
| `crates/ouroboros-core/src/graph/restore.rs` (new) | `restore_cyclic_ancestor_edges` + `has_path` |
| `crates/ouroboros-core/src/graph/mod.rs` | Add `pub mod restore;` |
| `crates/ouroboros-core/src/lib.rs` | Wire `restore_cyclic_ancestor_edges` into the pipeline |
| `crates/ouroboros-core/src/resolver/resolve.rs` (tests) | Add unit tests for suppressed edge recording |
| `crates/ouroboros-core/src/graph/restore.rs` (tests) | Add unit tests for restoration logic |
| `fixtures/ancestor_init_cycle/` (new) | Integration test fixture |
