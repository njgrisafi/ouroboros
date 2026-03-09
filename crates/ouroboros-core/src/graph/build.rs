use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use crate::discovery::DiscoveryResult;
use crate::resolver::ResolveResult;

/// A first-party file dependency graph.
///
/// - Key: relative file path of a first-party Python source file.
/// - Value: sorted, deduplicated set of first-party file paths that the key depends on.
pub type FileDependencyGraph = HashMap<PathBuf, BTreeSet<PathBuf>>;

/// Build the first-party file dependency graph from discovery and resolution results.
///
/// Every discovered first-party file appears as a node (even if it has no outgoing edges).
/// Resolved first-party dependency edges are translated from dotted module names back to
/// relative file paths using the discovery data.
pub fn build_file_dependency_graph(
    discovery: &DiscoveryResult,
    resolve_result: &ResolveResult,
) -> FileDependencyGraph {
    // Step 0: Build a module-name → rel_path lookup from the discovery results.
    let mut module_to_path: HashMap<&str, &PathBuf> = HashMap::new();
    for root in &discovery.roots {
        for file in &root.files {
            if !file.module_name.is_empty() {
                module_to_path.insert(&file.module_name, &file.rel_path);
            }
        }
    }

    // Step 1: Initialize every discovered file as a graph node with an empty dependency set.
    let mut graph: FileDependencyGraph = HashMap::new();
    for root in &discovery.roots {
        for file in &root.files {
            graph.entry(file.rel_path.clone()).or_default();
        }
    }

    // Step 2: Add dependency edges by mapping module names back to file paths.
    for dep in &resolve_result.deps {
        let from_path = module_to_path.get(dep.source.as_str()).cloned();
        let to_path = module_to_path.get(dep.target.as_str()).cloned();

        if let (Some(from), Some(to)) = (from_path, to_path) {
            graph.entry(from.clone()).or_default().insert(to.clone());
        }
    }

    graph
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{PythonFile, SourceRoot};
    use crate::resolver::ResolvedDep;

    /// Helper: build a `DiscoveryResult` from `(rel_path, module_name)` pairs.
    fn make_discovery(files: &[(&str, &str)]) -> DiscoveryResult {
        let python_files = files
            .iter()
            .map(|(path, module)| PythonFile {
                rel_path: PathBuf::from(path),
                module_name: module.to_string(),
            })
            .collect();

        DiscoveryResult {
            roots: vec![SourceRoot {
                path: PathBuf::from("/fake/root"),
                files: python_files,
            }],
        }
    }

    /// Helper: build a `ResolveResult` from `(source_module, target_module)` pairs.
    fn make_resolve(edges: &[(&str, &str)]) -> ResolveResult {
        let deps = edges
            .iter()
            .map(|(src, tgt)| ResolvedDep {
                source: src.to_string(),
                target: tgt.to_string(),
            })
            .collect();

        ResolveResult {
            deps,
            unresolved: Vec::new(),
        }
    }

    // Test case 1: A single module with no edges should still appear as a node.
    #[test]
    fn node_with_no_dependencies() {
        let discovery = make_discovery(&[("a.py", "a")]);
        let resolve = make_resolve(&[]);

        let graph = build_file_dependency_graph(&discovery, &resolve);

        assert!(graph.contains_key(&PathBuf::from("a.py")));
        assert!(graph[&PathBuf::from("a.py")].is_empty());
    }

    // Test case 2: A single edge produces the correct graph.
    #[test]
    fn single_edge() {
        let discovery = make_discovery(&[("a.py", "a"), ("b.py", "b")]);
        let resolve = make_resolve(&[("a", "b")]);

        let graph = build_file_dependency_graph(&discovery, &resolve);

        assert_eq!(graph.len(), 2);
        assert!(graph[&PathBuf::from("a.py")].contains(&PathBuf::from("b.py")));
        assert!(graph[&PathBuf::from("b.py")].is_empty());
    }

    // Test case 3: Duplicate edges are deduplicated by BTreeSet.
    #[test]
    fn duplicate_edges() {
        let discovery = make_discovery(&[("a.py", "a"), ("b.py", "b")]);
        let resolve = make_resolve(&[("a", "b"), ("a", "b")]);

        let graph = build_file_dependency_graph(&discovery, &resolve);

        assert_eq!(graph[&PathBuf::from("a.py")].len(), 1);
        assert!(graph[&PathBuf::from("a.py")].contains(&PathBuf::from("b.py")));
    }

    // Test case 4: Multiple dependencies are sorted deterministically.
    #[test]
    fn multiple_dependencies_sorted() {
        let discovery = make_discovery(&[
            ("a.py", "a"),
            ("b.py", "b"),
            ("c.py", "c"),
        ]);
        let resolve = make_resolve(&[("a", "c"), ("a", "b")]);

        let graph = build_file_dependency_graph(&discovery, &resolve);

        let deps: Vec<&PathBuf> = graph[&PathBuf::from("a.py")].iter().collect();
        assert_eq!(deps, vec![&PathBuf::from("b.py"), &PathBuf::from("c.py")]);
    }

    // Test case 5: A source node that appears only in edges (not in modules)
    // is handled defensively — the edge is silently skipped because we cannot
    // map the module name to a file path without discovery data.
    #[test]
    fn source_only_in_edges_is_skipped() {
        let discovery = make_discovery(&[("b.py", "b")]);
        // "a" is not in the discovery results.
        let resolve = make_resolve(&[("a", "b")]);

        let graph = build_file_dependency_graph(&discovery, &resolve);

        // Only "b.py" should be in the graph (from discovery).
        assert_eq!(graph.len(), 1);
        assert!(graph.contains_key(&PathBuf::from("b.py")));
    }
}
