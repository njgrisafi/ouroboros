use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use crate::discovery::DiscoveryResult;
use crate::resolver::ResolveResult;

/// A first-party file dependency graph.
///
/// - Key: relative file path of a first-party Python source file.
/// - Value: sorted, deduplicated set of first-party file paths that the key depends on.
pub type FileDependencyGraph = HashMap<PathBuf, BTreeSet<PathBuf>>;

pub struct EdgeMetadata {
    pub lines: HashMap<(PathBuf, PathBuf), Vec<u32>>,
}

pub struct FileGraphResult {
    pub graph: FileDependencyGraph,
    pub edge_metadata: EdgeMetadata,
}

pub fn build_file_dependency_graph(
    discovery: &DiscoveryResult,
    resolve_result: &ResolveResult,
) -> FileGraphResult {
    let mut module_to_path: HashMap<&str, &PathBuf> = HashMap::new();
    for root in &discovery.roots {
        for file in &root.files {
            if !file.module_name.is_empty() {
                module_to_path.insert(&file.module_name, &file.rel_path);
            }
        }
    }

    let mut graph: FileDependencyGraph = HashMap::new();
    for root in &discovery.roots {
        for file in &root.files {
            graph.entry(file.rel_path.clone()).or_default();
        }
    }

    let mut edge_lines: HashMap<(PathBuf, PathBuf), Vec<u32>> = HashMap::new();

    for dep in &resolve_result.deps {
        let from_path = module_to_path.get(dep.source.as_str()).cloned();
        let to_path = module_to_path.get(dep.target.as_str()).cloned();

        if let (Some(from), Some(to)) = (from_path, to_path) {
            graph.entry(from.clone()).or_default().insert(to.clone());
            edge_lines
                .entry((from.clone(), to.clone()))
                .or_default()
                .push(dep.line);
        }
    }

    FileGraphResult {
        graph,
        edge_metadata: EdgeMetadata { lines: edge_lines },
    }
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

    fn make_resolve(edges: &[(&str, &str)]) -> ResolveResult {
        let deps = edges
            .iter()
            .map(|(src, tgt)| ResolvedDep {
                source: src.to_string(),
                target: tgt.to_string(),
                line: 0,
            })
            .collect();

        ResolveResult {
            deps,
            unresolved: Vec::new(),
        }
    }

    #[test]
    fn node_with_no_dependencies() {
        let discovery = make_discovery(&[("a.py", "a")]);
        let resolve = make_resolve(&[]);

        let result = build_file_dependency_graph(&discovery, &resolve);
        let graph = result.graph;

        assert!(graph.contains_key(&PathBuf::from("a.py")));
        assert!(graph[&PathBuf::from("a.py")].is_empty());
    }

    #[test]
    fn single_edge() {
        let discovery = make_discovery(&[("a.py", "a"), ("b.py", "b")]);
        let resolve = make_resolve(&[("a", "b")]);

        let result = build_file_dependency_graph(&discovery, &resolve);
        let graph = result.graph;

        assert_eq!(graph.len(), 2);
        assert!(graph[&PathBuf::from("a.py")].contains(&PathBuf::from("b.py")));
        assert!(graph[&PathBuf::from("b.py")].is_empty());
    }

    #[test]
    fn duplicate_edges() {
        let discovery = make_discovery(&[("a.py", "a"), ("b.py", "b")]);
        let resolve = make_resolve(&[("a", "b"), ("a", "b")]);

        let result = build_file_dependency_graph(&discovery, &resolve);
        let graph = result.graph;

        assert_eq!(graph[&PathBuf::from("a.py")].len(), 1);
        assert!(graph[&PathBuf::from("a.py")].contains(&PathBuf::from("b.py")));
    }

    #[test]
    fn multiple_dependencies_sorted() {
        let discovery = make_discovery(&[("a.py", "a"), ("b.py", "b"), ("c.py", "c")]);
        let resolve = make_resolve(&[("a", "c"), ("a", "b")]);

        let result = build_file_dependency_graph(&discovery, &resolve);
        let graph = result.graph;

        let deps: Vec<&PathBuf> = graph[&PathBuf::from("a.py")].iter().collect();
        assert_eq!(deps, vec![&PathBuf::from("b.py"), &PathBuf::from("c.py")]);
    }

    #[test]
    fn source_only_in_edges_is_skipped() {
        let discovery = make_discovery(&[("b.py", "b")]);
        let resolve = make_resolve(&[("a", "b")]);

        let result = build_file_dependency_graph(&discovery, &resolve);
        let graph = result.graph;

        assert_eq!(graph.len(), 1);
        assert!(graph.contains_key(&PathBuf::from("b.py")));
    }
}
