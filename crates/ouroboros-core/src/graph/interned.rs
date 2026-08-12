//! Shared path interning for graph algorithms.
//!
//! [`InternedGraph`] is the single place where a [`FileDependencyGraph`] is
//! converted to dense `u32` node ids. Both Tarjan's SCC (`super::scc`) and
//! the impact BFS (`super::impact`) consume it, so the invariant that output
//! correctness relies on lives in exactly one place:
//!
//! **Ids are assigned in lexicographic path order, and adjacency lists
//! preserve the source `BTreeSet` iteration order (also lexicographic).**
//!
//! Iterating ids `0..n` therefore visits nodes in lexicographic path order,
//! and comparing `Vec<u32>` id-paths lexicographically is equivalent to
//! comparing the corresponding paths — keeping traversal order and output
//! identical to the original `PathBuf`-keyed algorithms.

use std::path::PathBuf;

use rustc_hash::FxHashMap;

use super::FileDependencyGraph;

/// Sentinel for "no node" / "not visited" in interned id arrays.
pub(crate) const NO_NODE: u32 = u32::MAX;

/// An interned view of a [`FileDependencyGraph`]: every node gets a dense
/// `u32` id and adjacency lists are `Vec`-indexed by id, so graph algorithms
/// run without hashing or cloning paths. See the module docs for the
/// ordering invariant.
pub struct InternedGraph<'a> {
    paths: Vec<&'a PathBuf>,
    ids: FxHashMap<&'a PathBuf, u32>,
    /// Adjacency by node id, in source `BTreeSet` (lexicographic) order.
    pub(crate) adjacency: Vec<Vec<u32>>,
}

impl<'a> InternedGraph<'a> {
    pub fn new(graph: &'a FileDependencyGraph) -> Self {
        // Intern graph keys plus any dangling deps (present in an edge set
        // but not a key) as leaf nodes, mirroring how the PathBuf-keyed
        // algorithms tolerate them.
        let mut paths: Vec<&PathBuf> = graph.keys().collect();
        {
            let mut seen: FxHashMap<&PathBuf, ()> = graph.keys().map(|key| (key, ())).collect();
            for deps in graph.values() {
                for dep in deps {
                    if !seen.contains_key(dep) {
                        seen.insert(dep, ());
                        paths.push(dep);
                    }
                }
            }
        }
        paths.sort();

        let ids: FxHashMap<&PathBuf, u32> = paths
            .iter()
            .enumerate()
            .map(|(i, path)| (*path, i as u32))
            .collect();
        let adjacency = paths
            .iter()
            .map(|path| {
                graph
                    .get(*path)
                    .map(|deps| deps.iter().map(|dep| ids[dep]).collect())
                    .unwrap_or_default()
            })
            .collect();

        InternedGraph {
            paths,
            ids,
            adjacency,
        }
    }

    /// Look up the interned id for a path, if it is a graph node.
    pub fn id(&self, path: &PathBuf) -> Option<u32> {
        self.ids.get(path).copied()
    }

    /// Map an interned id back to its path.
    pub fn path(&self, id: u32) -> &'a PathBuf {
        self.paths[id as usize]
    }

    pub fn len(&self) -> usize {
        self.paths.len()
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn make_graph(edges: &[(&str, &[&str])]) -> FileDependencyGraph {
        let mut graph = FileDependencyGraph::default();
        for (node, deps) in edges {
            let dep_set: BTreeSet<PathBuf> = deps.iter().map(PathBuf::from).collect();
            graph.insert(PathBuf::from(node), dep_set);
        }
        graph
    }

    #[test]
    fn ids_are_assigned_in_lexicographic_order() {
        let graph = make_graph(&[("z.py", &[]), ("a.py", &[]), ("m.py", &[])]);
        let igraph = InternedGraph::new(&graph);

        assert_eq!(igraph.id(&PathBuf::from("a.py")), Some(0));
        assert_eq!(igraph.id(&PathBuf::from("m.py")), Some(1));
        assert_eq!(igraph.id(&PathBuf::from("z.py")), Some(2));
        assert_eq!(igraph.path(0), &PathBuf::from("a.py"));
    }

    #[test]
    fn dangling_dep_becomes_leaf_node() {
        let graph = make_graph(&[("a.py", &["ext.py"])]);
        let igraph = InternedGraph::new(&graph);

        let ext = igraph.id(&PathBuf::from("ext.py")).expect("dangling dep");
        assert!(igraph.adjacency[ext as usize].is_empty());
        let a = igraph.id(&PathBuf::from("a.py")).unwrap();
        assert_eq!(igraph.adjacency[a as usize], vec![ext]);
    }

    #[test]
    fn adjacency_preserves_lexicographic_iteration_order() {
        let graph = make_graph(&[("a.py", &["z.py", "b.py", "m.py"])]);
        let igraph = InternedGraph::new(&graph);

        let a = igraph.id(&PathBuf::from("a.py")).unwrap();
        let dep_paths: Vec<&PathBuf> = igraph.adjacency[a as usize]
            .iter()
            .map(|&id| igraph.path(id))
            .collect();
        assert_eq!(
            dep_paths,
            vec![
                &PathBuf::from("b.py"),
                &PathBuf::from("m.py"),
                &PathBuf::from("z.py")
            ]
        );
    }
}
