use std::collections::HashMap;
use std::path::PathBuf;

use crate::resolver::SuppressedAncestorEdge;

use super::{EdgeMetadata, FileDependencyGraph, strongly_connected_components};

/// Restore suppressed ancestor-init edges that participate in a cycle.
///
/// Candidates are PROPER-ancestor suppressions only (`source != ancestor`,
/// guaranteed by T1's recording rule; this fn also defensively skips any
/// `from == to` pair). An edge `source -> ancestor_package` is restored iff,
/// once ALL candidate suppressed edges are added, `source` and
/// `ancestor_package` fall in the same strongly connected component (i.e. the
/// edge closes a real cycle). For proper-ancestor candidates this "same SCC"
/// test is exactly equivalent to "ancestor reaches source in the augmented
/// graph" and is fix-point-complete for mutually dependent candidates.
/// Non-cyclic suppressed edges are left out (they are runtime-real but add
/// no cycle and would only be noise for the detector).
///
/// Performance note: clones the graph to build the augmented version. This
/// runs only when the user opts in via `--include-self-ancestor-init`. An
/// in-place insert-then-prune is a possible optimization if profiling flags
/// the clone on very large graphs.
pub fn restore_self_ancestor_init_edges(
    graph: &mut FileDependencyGraph,
    edge_metadata: &mut EdgeMetadata,
    module_to_path: &HashMap<String, PathBuf>,
    suppressed: &[SuppressedAncestorEdge],
) {
    // 1. Map to path pairs; skip edges whose endpoints aren't graph nodes,
    //    and defensively drop any self pair (from == to) — a self-loop is
    //    never a real ancestor-init cycle (guards against module-name
    //    collisions mapping two modules onto the same path).
    let candidates: Vec<(PathBuf, PathBuf, u32)> = suppressed
        .iter()
        .filter_map(|e| {
            let from = module_to_path.get(&e.source)?;
            let to = module_to_path.get(&e.ancestor_package)?;
            if from == to {
                return None;
            }
            Some((from.clone(), to.clone(), e.line))
        })
        .collect();
    if candidates.is_empty() {
        return;
    }

    // 2. Augmented graph = clone + all candidate edges.
    let mut augmented = graph.clone();
    for (from, to, _) in &candidates {
        augmented.entry(from.clone()).or_default().insert(to.clone());
    }

    // 3. One SCC pass; build node -> scc-id map.
    let sccs = strongly_connected_components(&augmented);
    let mut node_to_scc: HashMap<&PathBuf, usize> = HashMap::new();
    for (id, scc) in sccs.iter().enumerate() {
        for node in scc {
            node_to_scc.insert(node, id);
        }
    }

    // 4. Keep candidates whose endpoints share an SCC.
    for (from, to, line) in candidates {
        if node_to_scc.contains_key(&from) && node_to_scc.get(&from) == node_to_scc.get(&to) {
            graph.entry(from.clone()).or_default().insert(to.clone());
            edge_metadata
                .lines
                .entry((from, to))
                .or_default()
                .push(line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// Build a `FileDependencyGraph` from `(node, &[dep])` pairs.
    fn make_graph(edges: &[(&str, &[&str])]) -> FileDependencyGraph {
        let mut graph = FileDependencyGraph::new();
        for (node, deps) in edges {
            let dep_set: BTreeSet<PathBuf> = deps.iter().map(PathBuf::from).collect();
            graph.insert(PathBuf::from(node), dep_set);
        }
        graph
    }

    /// Build a `module_to_path` map from `(module, path)` pairs.
    fn make_module_to_path(pairs: &[(&str, &str)]) -> HashMap<String, PathBuf> {
        pairs
            .iter()
            .map(|(module, path)| (module.to_string(), PathBuf::from(path)))
            .collect()
    }

    /// Build a suppressed-edge slice from `(source, ancestor, line)` triples.
    fn make_suppressed(edges: &[(&str, &str, u32)]) -> Vec<SuppressedAncestorEdge> {
        edges
            .iter()
            .map(|(source, ancestor, line)| SuppressedAncestorEdge {
                source: source.to_string(),
                ancestor_package: ancestor.to_string(),
                line: *line,
            })
            .collect()
    }

    // Test 1: a direct source->ancestor suppression that closes a 2-node cycle
    // is restored, with its line recorded in edge metadata.
    #[test]
    fn restores_direct_cycle() {
        let mut graph = make_graph(&[("pkg/__init__.py", &["pkg/child.py"]), ("pkg/child.py", &[])]);
        let mut edge_metadata = EdgeMetadata {
            lines: HashMap::new(),
        };
        let module_to_path =
            make_module_to_path(&[("pkg", "pkg/__init__.py"), ("pkg.child", "pkg/child.py")]);
        let suppressed = make_suppressed(&[("pkg.child", "pkg", 5)]);

        restore_self_ancestor_init_edges(
            &mut graph,
            &mut edge_metadata,
            &module_to_path,
            &suppressed,
        );

        assert!(
            graph[&PathBuf::from("pkg/child.py")].contains(&PathBuf::from("pkg/__init__.py")),
            "child -> __init__ edge should be restored"
        );
        let lines = edge_metadata
            .lines
            .get(&(PathBuf::from("pkg/child.py"), PathBuf::from("pkg/__init__.py")))
            .expect("edge metadata should record the restored edge");
        assert!(lines.contains(&5), "line 5 should be recorded");
    }

    // Test 2: a suppressed edge whose ancestor cannot reach the source (no
    // cycle) is left out of the graph.
    #[test]
    fn leaves_non_cyclic_suppressed_edge() {
        // pkg/__init__.py is NOT in the graph — no path from it to child.
        let mut graph = make_graph(&[("pkg/child.py", &["pkg/other.py"]), ("pkg/other.py", &[])]);
        let mut edge_metadata = EdgeMetadata {
            lines: HashMap::new(),
        };
        let module_to_path =
            make_module_to_path(&[("pkg", "pkg/__init__.py"), ("pkg.child", "pkg/child.py")]);
        let suppressed = make_suppressed(&[("pkg.child", "pkg", 1)]);

        restore_self_ancestor_init_edges(
            &mut graph,
            &mut edge_metadata,
            &module_to_path,
            &suppressed,
        );

        assert!(
            !graph[&PathBuf::from("pkg/child.py")].contains(&PathBuf::from("pkg/__init__.py")),
            "non-cyclic suppressed edge should NOT be restored"
        );
    }

    // Test 3: a suppressed edge that closes an indirect cycle (ancestor reaches
    // source through intermediate nodes) is restored.
    #[test]
    fn restores_indirect_cycle() {
        let mut graph = make_graph(&[
            ("parent/__init__.py", &["a.py"]),
            ("a.py", &["b.py"]),
            ("b.py", &[]),
        ]);
        let mut edge_metadata = EdgeMetadata {
            lines: HashMap::new(),
        };
        let module_to_path =
            make_module_to_path(&[("pkg", "parent/__init__.py"), ("pkg.b", "b.py")]);
        let suppressed = make_suppressed(&[("pkg.b", "pkg", 3)]);

        restore_self_ancestor_init_edges(
            &mut graph,
            &mut edge_metadata,
            &module_to_path,
            &suppressed,
        );

        assert!(
            graph[&PathBuf::from("b.py")].contains(&PathBuf::from("parent/__init__.py")),
            "b -> parent/__init__ edge should be restored (parent reaches b via a)"
        );
    }

    // Test 4: two suppressed edges that are each cyclic only because both are
    // present (mutually dependent) are BOTH restored in one SCC pass. Each
    // init file imports the OTHER package's submodule, so the augmented graph
    // a/x -> a/__init__ -> b/y -> b/__init__ -> a/x is one SCC; dropping either
    // candidate breaks the loop (proven separately in
    // `interdependent_edges_are_not_restored_alone`).
    #[test]
    fn restores_interdependent_edges() {
        let mut graph = make_graph(&[
            ("a/__init__.py", &["b/y.py"]),
            ("b/__init__.py", &["a/x.py"]),
        ]);
        let mut edge_metadata = EdgeMetadata {
            lines: HashMap::new(),
        };
        let module_to_path = make_module_to_path(&[
            ("a", "a/__init__.py"),
            ("a.x", "a/x.py"),
            ("b", "b/__init__.py"),
            ("b.y", "b/y.py"),
        ]);
        let suppressed = make_suppressed(&[("a.x", "a", 1), ("b.y", "b", 2)]);

        restore_self_ancestor_init_edges(
            &mut graph,
            &mut edge_metadata,
            &module_to_path,
            &suppressed,
        );

        assert!(
            graph[&PathBuf::from("a/x.py")].contains(&PathBuf::from("a/__init__.py")),
            "a/x.py -> a/__init__ edge should be restored"
        );
        assert!(
            graph[&PathBuf::from("b/y.py")].contains(&PathBuf::from("b/__init__.py")),
            "b/y.py -> b/__init__ edge should be restored"
        );
    }

    // Test 4 companion: proves the interdependence — with only ONE of the two
    // mutually dependent candidates present, that candidate does not close a
    // cycle and is left out.
    #[test]
    fn interdependent_edges_are_not_restored_alone() {
        let mut graph = make_graph(&[
            ("a/__init__.py", &["b/y.py"]),
            ("b/__init__.py", &["a/x.py"]),
        ]);
        let mut edge_metadata = EdgeMetadata {
            lines: HashMap::new(),
        };
        let module_to_path = make_module_to_path(&[
            ("a", "a/__init__.py"),
            ("a.x", "a/x.py"),
            ("b", "b/__init__.py"),
            ("b.y", "b/y.py"),
        ]);
        let suppressed = make_suppressed(&[("a.x", "a", 1)]);

        restore_self_ancestor_init_edges(
            &mut graph,
            &mut edge_metadata,
            &module_to_path,
            &suppressed,
        );

        assert!(
            !graph
                .get(&PathBuf::from("a/x.py"))
                .is_some_and(|deps| deps.contains(&PathBuf::from("a/__init__.py"))),
            "a single candidate that does not close a cycle must not be restored"
        );
    }

    // Test 5: an empty suppressed slice leaves the graph untouched.
    #[test]
    fn noop_on_empty_suppressed() {
        let mut graph = make_graph(&[("a.py", &["b.py"]), ("b.py", &["a.py"])]);
        let original = graph.clone();
        let mut edge_metadata = EdgeMetadata {
            lines: HashMap::new(),
        };
        let module_to_path = make_module_to_path(&[("a", "a.py"), ("b", "b.py")]);
        let suppressed: Vec<SuppressedAncestorEdge> = Vec::new();

        restore_self_ancestor_init_edges(
            &mut graph,
            &mut edge_metadata,
            &module_to_path,
            &suppressed,
        );

        assert_eq!(graph, original, "empty suppressed slice must not mutate graph");
        assert!(edge_metadata.lines.is_empty(), "no metadata should be added");
    }

    // Test 6: a suppressed edge whose source has no mapping in module_to_path
    // is skipped without panicking and leaves the graph unchanged.
    #[test]
    fn skips_unknown_module_paths() {
        let mut graph = make_graph(&[("pkg/__init__.py", &["pkg/child.py"]), ("pkg/child.py", &[])]);
        let original = graph.clone();
        let mut edge_metadata = EdgeMetadata {
            lines: HashMap::new(),
        };
        // "pkg.unknown" is intentionally absent from module_to_path.
        let module_to_path = make_module_to_path(&[("pkg", "pkg/__init__.py")]);
        let suppressed = make_suppressed(&[("pkg.unknown", "pkg", 7)]);

        restore_self_ancestor_init_edges(
            &mut graph,
            &mut edge_metadata,
            &module_to_path,
            &suppressed,
        );

        assert_eq!(graph, original, "unknown module edge must not mutate graph");
        assert!(edge_metadata.lines.is_empty(), "no metadata should be added");
    }

    // Test 7: a candidate whose endpoints map to the same path (from == to) is
    // dropped in step 1 — no self-loop edge and no metadata entry.
    #[test]
    fn self_candidate_is_not_restored() {
        let mut graph = make_graph(&[("pkg/__init__.py", &[])]);
        let mut edge_metadata = EdgeMetadata {
            lines: HashMap::new(),
        };
        // Both source and ancestor resolve to the same path.
        let module_to_path = make_module_to_path(&[("pkg", "pkg/__init__.py")]);
        let suppressed = make_suppressed(&[("pkg", "pkg", 1)]);

        restore_self_ancestor_init_edges(
            &mut graph,
            &mut edge_metadata,
            &module_to_path,
            &suppressed,
        );

        assert!(
            !graph[&PathBuf::from("pkg/__init__.py")].contains(&PathBuf::from("pkg/__init__.py")),
            "self-loop edge must not be added"
        );
        assert!(
            !edge_metadata
                .lines
                .contains_key(&(PathBuf::from("pkg/__init__.py"), PathBuf::from("pkg/__init__.py"))),
            "no metadata entry for a self pair"
        );
    }
}
