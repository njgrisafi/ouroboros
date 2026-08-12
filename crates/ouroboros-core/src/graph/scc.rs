use std::path::PathBuf;

use super::FileDependencyGraph;
use super::interned::{InternedGraph, NO_NODE};

/// A single dependency cycle: a sorted list of file paths that form a cycle.
pub type FileCycle = Vec<PathBuf>;

/// Sentinel for "not yet visited" in the Tarjan index array.
const UNVISITED: u32 = NO_NODE;

/// Compute all strongly connected components of the file dependency graph
/// using Tarjan's algorithm.
///
/// Returns a deterministic list of SCCs:
/// - each SCC's members are sorted lexicographically by path
/// - the list of SCCs is sorted by the first member of each SCC
///
/// Includes singleton SCCs (size 1), even those without self-loops.
///
/// Internally the graph is interned to `u32` node ids and the algorithm is
/// iterative, so deep import chains cannot overflow the call stack.
pub fn strongly_connected_components(graph: &FileDependencyGraph) -> Vec<Vec<PathBuf>> {
    // Interning assigns ids in lexicographic path order, so the traversal
    // visits roots in the same order as the original PathBuf-keyed version.
    let igraph = InternedGraph::new(graph);
    let components = tarjan_scc(&igraph.adjacency);

    // Map back to paths and impose the deterministic output order.
    let mut result: Vec<Vec<PathBuf>> = components
        .into_iter()
        .map(|component| {
            component
                .into_iter()
                .map(|id| igraph.path(id).clone())
                .collect()
        })
        .collect();
    for component in &mut result {
        component.sort();
    }
    result.sort_by(|a, b| a[0].cmp(&b[0]));
    result
}

/// Iterative Tarjan's SCC over a `Vec`-indexed adjacency list.
///
/// Returns components in reverse topological discovery order (unsorted);
/// callers sort as needed. `indices`, `lowlinks`, and the explicit call
/// stack are all `Vec`-indexed by node id — no hashing in the hot loop.
fn tarjan_scc(adjacency: &[Vec<u32>]) -> Vec<Vec<u32>> {
    let node_count = adjacency.len();
    let mut indices = vec![UNVISITED; node_count];
    let mut lowlinks = vec![0u32; node_count];
    let mut on_stack = vec![false; node_count];
    let mut tarjan_stack: Vec<u32> = Vec::new();
    let mut components: Vec<Vec<u32>> = Vec::new();
    let mut next_index = 0u32;

    // Explicit DFS call stack: (node, next child position to process).
    let mut call_stack: Vec<(u32, usize)> = Vec::new();

    for root in 0..node_count as u32 {
        if indices[root as usize] != UNVISITED {
            continue;
        }

        indices[root as usize] = next_index;
        lowlinks[root as usize] = next_index;
        next_index += 1;
        tarjan_stack.push(root);
        on_stack[root as usize] = true;
        call_stack.push((root, 0));

        while let Some(&mut (node, ref mut child_pos)) = call_stack.last_mut() {
            let deps = &adjacency[node as usize];
            if *child_pos < deps.len() {
                let dep = deps[*child_pos];
                *child_pos += 1;

                if indices[dep as usize] == UNVISITED {
                    // Tree edge: descend into dep.
                    indices[dep as usize] = next_index;
                    lowlinks[dep as usize] = next_index;
                    next_index += 1;
                    tarjan_stack.push(dep);
                    on_stack[dep as usize] = true;
                    call_stack.push((dep, 0));
                } else if on_stack[dep as usize] {
                    // Back edge to a node on the Tarjan stack.
                    let node_lowlink = &mut lowlinks[node as usize];
                    *node_lowlink = (*node_lowlink).min(indices[dep as usize]);
                }
                // Cross edge to a finished SCC: ignored.
            } else {
                // All deps processed: finish this node.
                if lowlinks[node as usize] == indices[node as usize] {
                    let mut component = Vec::new();
                    loop {
                        let w = tarjan_stack.pop().expect("Tarjan stack underflow");
                        on_stack[w as usize] = false;
                        let is_root = w == node;
                        component.push(w);
                        if is_root {
                            break;
                        }
                    }
                    components.push(component);
                }
                call_stack.pop();
                // Propagate lowlink to the parent frame, if any.
                if let Some(&(parent, _)) = call_stack.last() {
                    let child_lowlink = lowlinks[node as usize];
                    let parent_lowlink = &mut lowlinks[parent as usize];
                    *parent_lowlink = (*parent_lowlink).min(child_lowlink);
                }
            }
        }
    }

    components
}

/// Compute dependency cycles from the file dependency graph.
///
/// Calls [`strongly_connected_components`] and filters to only real cycles:
/// - SCCs with more than one member
/// - SCCs with exactly one member that has a self-loop
pub fn dependency_cycles(graph: &FileDependencyGraph) -> Vec<FileCycle> {
    cycles_from_sccs(graph, &strongly_connected_components(graph))
}

/// Filter precomputed SCCs down to real cycles, cloning only the kept SCCs.
///
/// Use this when the caller already needs the full SCC list (e.g. to map
/// nodes to SCCs for trace analysis) so Tarjan's algorithm runs only once.
pub fn cycles_from_sccs(graph: &FileDependencyGraph, sccs: &[Vec<PathBuf>]) -> Vec<FileCycle> {
    sccs.iter()
        .filter(|scc| scc.len() > 1 || (scc.len() == 1 && has_self_loop(graph, &scc[0])))
        .cloned()
        .collect()
}

/// Check whether a node has a self-loop (depends on itself).
fn has_self_loop(graph: &FileDependencyGraph, node: &PathBuf) -> bool {
    graph
        .get(node)
        .map(|deps| deps.contains(node))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::Path;

    /// Helper: build a `FileDependencyGraph` from `(node, &[dep])` pairs.
    fn make_graph(edges: &[(&str, &[&str])]) -> FileDependencyGraph {
        let mut graph = FileDependencyGraph::default();
        for (node, deps) in edges {
            let dep_set: BTreeSet<PathBuf> = deps.iter().map(PathBuf::from).collect();
            graph.insert(PathBuf::from(node), dep_set);
        }
        graph
    }

    // Test 1: acyclic chain — no cycles.
    #[test]
    fn acyclic_chain() {
        let graph = make_graph(&[("a.py", &["b.py"]), ("b.py", &["c.py"]), ("c.py", &[])]);

        let sccs = strongly_connected_components(&graph);
        assert_eq!(sccs.len(), 3);
        // Each SCC should be a singleton.
        for scc in &sccs {
            assert_eq!(scc.len(), 1);
        }

        let cycles = dependency_cycles(&graph);
        assert!(cycles.is_empty());
    }

    // Test 2: 2-node cycle.
    #[test]
    fn two_node_cycle() {
        let graph = make_graph(&[("a.py", &["b.py"]), ("b.py", &["a.py"])]);

        let sccs = strongly_connected_components(&graph);
        let big: Vec<_> = sccs.iter().filter(|s| s.len() > 1).collect();
        assert_eq!(big.len(), 1);
        assert_eq!(big[0], &vec![PathBuf::from("a.py"), PathBuf::from("b.py")]);

        let cycles = dependency_cycles(&graph);
        assert_eq!(cycles.len(), 1);
        assert_eq!(
            cycles[0],
            vec![PathBuf::from("a.py"), PathBuf::from("b.py")]
        );
    }

    // Test 3: 3-node cycle.
    #[test]
    fn three_node_cycle() {
        let graph = make_graph(&[
            ("a.py", &["b.py"]),
            ("b.py", &["c.py"]),
            ("c.py", &["a.py"]),
        ]);

        let sccs = strongly_connected_components(&graph);
        assert_eq!(sccs.len(), 1);
        assert_eq!(sccs[0].len(), 3);

        let cycles = dependency_cycles(&graph);
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].len(), 3);
    }

    // Test 4: self-loop is a cycle.
    #[test]
    fn self_loop() {
        let graph = make_graph(&[("a.py", &["a.py"])]);

        let sccs = strongly_connected_components(&graph);
        assert_eq!(sccs.len(), 1);
        assert_eq!(sccs[0], vec![PathBuf::from("a.py")]);

        let cycles = dependency_cycles(&graph);
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0], vec![PathBuf::from("a.py")]);
    }

    // Test 5: singleton without self-loop is NOT a cycle.
    #[test]
    fn singleton_no_self_loop() {
        let graph = make_graph(&[("a.py", &[])]);

        let sccs = strongly_connected_components(&graph);
        assert_eq!(sccs.len(), 1);

        let cycles = dependency_cycles(&graph);
        assert!(cycles.is_empty());
    }

    // Test 6: mixed graph with cycles and non-cycles.
    #[test]
    fn mixed_graph() {
        let graph = make_graph(&[
            ("a.py", &["b.py"]),
            ("b.py", &["a.py"]),
            ("c.py", &["d.py"]),
            ("d.py", &[]),
            ("e.py", &["e.py"]),
        ]);

        let sccs = strongly_connected_components(&graph);
        assert_eq!(sccs.len(), 4); // [a,b], [c], [d], [e]

        let cycles = dependency_cycles(&graph);
        assert_eq!(cycles.len(), 2);
        assert_eq!(
            cycles[0],
            vec![PathBuf::from("a.py"), PathBuf::from("b.py")]
        );
        assert_eq!(cycles[1], vec![PathBuf::from("e.py")]);
    }

    // Test 7: deterministic ordering regardless of insertion order.
    #[test]
    fn deterministic_ordering() {
        // Insert nodes in reverse order.
        let graph = make_graph(&[
            ("z.py", &["y.py"]),
            ("y.py", &["z.py"]),
            ("m.py", &["m.py"]),
            ("a.py", &[]),
        ]);

        let sccs = strongly_connected_components(&graph);

        // SCCs should be sorted by first member.
        assert_eq!(sccs[0], vec![PathBuf::from("a.py")]);
        assert_eq!(sccs[1], vec![PathBuf::from("m.py")]);
        assert_eq!(sccs[2], vec![PathBuf::from("y.py"), PathBuf::from("z.py")]);

        let cycles = dependency_cycles(&graph);
        assert_eq!(cycles.len(), 2);
        assert_eq!(cycles[0], vec![PathBuf::from("m.py")]);
        assert_eq!(
            cycles[1],
            vec![PathBuf::from("y.py"), PathBuf::from("z.py")]
        );
    }

    // Test 8: deep chain does not overflow the stack (iterative Tarjan).
    #[test]
    fn deep_chain_no_stack_overflow() {
        let depth = 100_000;
        let mut graph = FileDependencyGraph::default();
        for i in 0..depth {
            let node = PathBuf::from(format!("m{i:07}.py"));
            let mut deps = BTreeSet::new();
            if i + 1 < depth {
                deps.insert(PathBuf::from(format!("m{:07}.py", i + 1)));
            }
            graph.insert(node, deps);
        }
        // One back edge from the deepest node to the root makes the whole
        // chain a single SCC — the worst case for recursion depth.
        graph
            .get_mut(Path::new(&format!("m{:07}.py", depth - 1)))
            .unwrap()
            .insert(PathBuf::from("m0000000.py"));

        let sccs = strongly_connected_components(&graph);
        assert_eq!(sccs.len(), 1);
        assert_eq!(sccs[0].len(), depth);
    }

    // Test 9: dep that is not a graph key is treated as a leaf node.
    #[test]
    fn dangling_dep_is_leaf() {
        let graph = make_graph(&[("a.py", &["ext.py"])]);
        let sccs = strongly_connected_components(&graph);
        assert_eq!(sccs.len(), 2);
        assert_eq!(sccs[0], vec![PathBuf::from("a.py")]);
        assert_eq!(sccs[1], vec![PathBuf::from("ext.py")]);
    }
}
