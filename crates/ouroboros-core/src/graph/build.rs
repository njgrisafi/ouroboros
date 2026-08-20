use std::collections::BTreeSet;
use std::path::PathBuf;

use rustc_hash::FxHashMap;

use crate::discovery::DiscoveryResult;
use crate::resolver::ResolveResult;

pub type FileDependencyGraph = FxHashMap<PathBuf, BTreeSet<PathBuf>>;

pub struct EdgeMetadata {
    pub lines: FxHashMap<(PathBuf, PathBuf), Vec<u32>>,
}

pub struct FileGraphResult {
    pub graph: FileDependencyGraph,
    pub edge_metadata: EdgeMetadata,
    pub module_collisions: Vec<(String, Vec<PathBuf>)>,
}

pub fn build_file_dependency_graph(
    discovery: &DiscoveryResult,
    resolve_result: &ResolveResult,
) -> FileGraphResult {
    let mut module_to_path: FxHashMap<&str, &PathBuf> = FxHashMap::default();
    let mut collisions: FxHashMap<&str, Vec<&PathBuf>> = FxHashMap::default();

    for root in &discovery.roots {
        for file in &root.files {
            if !file.module_name.is_empty() {
                if let Some(existing) = module_to_path
                    .get(file.module_name.as_str())
                    .filter(|e| **e != &file.rel_path)
                {
                    let entry = collisions
                        .entry(&file.module_name)
                        .or_insert_with(|| vec![existing]);
                    if !entry.contains(&&file.rel_path) {
                        entry.push(&file.rel_path);
                    }
                }
                module_to_path.insert(&file.module_name, &file.rel_path);
            }
        }
    }

    let module_collisions: Vec<(String, Vec<PathBuf>)> = collisions
        .into_iter()
        .map(|(name, paths)| {
            let mut sorted = paths.into_iter().cloned().collect::<Vec<_>>();
            sorted.sort();
            (name.to_string(), sorted)
        })
        .collect();

    let mut graph: FileDependencyGraph = FxHashMap::default();
    for root in &discovery.roots {
        for file in &root.files {
            graph.entry(file.rel_path.clone()).or_default();
        }
    }

    let mut edge_lines: FxHashMap<(PathBuf, PathBuf), Vec<u32>> = FxHashMap::default();

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
        module_collisions,
    }
}

/// Restrict `graph` to edges whose endpoints are both in `members`.
pub fn induced_subgraph_on(
    graph: &FileDependencyGraph,
    members: &BTreeSet<PathBuf>,
) -> FileDependencyGraph {
    let mut result = FileDependencyGraph::default();
    for member in members {
        let deps = graph
            .get(member)
            .into_iter()
            .flat_map(|targets| targets.iter())
            .filter(|target| members.contains(*target))
            .cloned()
            .collect();
        result.insert(member.clone(), deps);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{PythonFile, SourceRoot};
    use crate::resolver::ResolvedDep;

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

    fn make_discovery_two_roots(
        root1_files: &[(&str, &str)],
        root2_files: &[(&str, &str)],
    ) -> DiscoveryResult {
        DiscoveryResult {
            roots: vec![
                SourceRoot {
                    path: PathBuf::from("/fake/src"),
                    files: root1_files
                        .iter()
                        .map(|(p, m)| PythonFile {
                            rel_path: PathBuf::from(p),
                            module_name: m.to_string(),
                        })
                        .collect(),
                },
                SourceRoot {
                    path: PathBuf::from("/fake/lib"),
                    files: root2_files
                        .iter()
                        .map(|(p, m)| PythonFile {
                            rel_path: PathBuf::from(p),
                            module_name: m.to_string(),
                        })
                        .collect(),
                },
            ],
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
            suppressed_ancestor_edges: Vec::new(),
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

    #[test]
    fn cross_root_same_module_name_yields_collision() {
        let discovery = make_discovery_two_roots(
            &[("src/utils/helper.py", "utils.helper")],
            &[("lib/utils/helper.py", "utils.helper")],
        );
        let resolve = make_resolve(&[]);

        let result = build_file_dependency_graph(&discovery, &resolve);
        assert_eq!(result.module_collisions.len(), 1);
        assert_eq!(result.module_collisions[0].0, "utils.helper");
        assert_eq!(result.module_collisions[0].1.len(), 2);
    }

    #[test]
    fn distinct_module_names_yield_no_collision() {
        let discovery =
            make_discovery_two_roots(&[("src/app.py", "app")], &[("lib/utils.py", "utils")]);
        let resolve = make_resolve(&[]);

        let result = build_file_dependency_graph(&discovery, &resolve);
        assert!(result.module_collisions.is_empty());
    }

    #[test]
    fn induced_subgraph_on_keeps_edges_within_members() {
        let mut graph = FileDependencyGraph::default();
        graph.insert(
            PathBuf::from("a.py"),
            BTreeSet::from([PathBuf::from("b.py")]),
        );
        graph.insert(
            PathBuf::from("b.py"),
            BTreeSet::from([PathBuf::from("c.py")]),
        );
        graph.insert(PathBuf::from("c.py"), BTreeSet::new());

        let members = BTreeSet::from([PathBuf::from("a.py"), PathBuf::from("b.py")]);
        let subgraph = induced_subgraph_on(&graph, &members);

        assert_eq!(
            subgraph.get(&PathBuf::from("a.py")).unwrap(),
            &BTreeSet::from([PathBuf::from("b.py")])
        );
        assert_eq!(
            subgraph.get(&PathBuf::from("b.py")).unwrap(),
            &BTreeSet::new()
        );
        assert!(!subgraph.contains_key(&PathBuf::from("c.py")));
    }
}
