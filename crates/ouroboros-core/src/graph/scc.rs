use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::FileDependencyGraph;

/// A single dependency cycle: a sorted list of file paths that form a cycle.
pub type FileCycle = Vec<PathBuf>;

/// Internal state for Tarjan's SCC algorithm.
struct TarjanState {
    index: usize,
    indices: HashMap<PathBuf, usize>,
    lowlinks: HashMap<PathBuf, usize>,
    stack: Vec<PathBuf>,
    on_stack: HashSet<PathBuf>,
    components: Vec<Vec<PathBuf>>,
}

/// Compute all strongly connected components of the file dependency graph
/// using Tarjan's algorithm.
///
/// Returns a deterministic list of SCCs:
/// - each SCC's members are sorted lexicographically by path
/// - the list of SCCs is sorted by the first member of each SCC
///
/// Includes singleton SCCs (size 1), even those without self-loops.
pub fn strongly_connected_components(graph: &FileDependencyGraph) -> Vec<Vec<PathBuf>> {
    let mut state = TarjanState {
        index: 0,
        indices: HashMap::new(),
        lowlinks: HashMap::new(),
        stack: Vec::new(),
        on_stack: HashSet::new(),
        components: Vec::new(),
    };

    // Visit every node in a deterministic order.
    let mut nodes: Vec<&PathBuf> = graph.keys().collect();
    nodes.sort();

    for node in nodes {
        if !state.indices.contains_key(node) {
            strongconnect(node, graph, &mut state);
        }
    }

    // Sort each SCC internally, then sort the list of SCCs by first member.
    for component in &mut state.components {
        component.sort();
    }
    state.components.sort_by(|a, b| a[0].cmp(&b[0]));

    state.components
}

/// Compute dependency cycles from the file dependency graph.
///
/// Calls [`strongly_connected_components`] and filters to only real cycles:
/// - SCCs with more than one member
/// - SCCs with exactly one member that has a self-loop
pub fn dependency_cycles(graph: &FileDependencyGraph) -> Vec<FileCycle> {
    strongly_connected_components(graph)
        .into_iter()
        .filter(|scc| scc.len() > 1 || (scc.len() == 1 && has_self_loop(graph, &scc[0])))
        .collect()
}

/// Recursive Tarjan strongconnect for a single node.
fn strongconnect(node: &PathBuf, graph: &FileDependencyGraph, state: &mut TarjanState) {
    let idx = state.index;
    state.indices.insert(node.clone(), idx);
    state.lowlinks.insert(node.clone(), idx);
    state.index += 1;
    state.stack.push(node.clone());
    state.on_stack.insert(node.clone());

    // Visit dependencies.
    if let Some(deps) = graph.get(node) {
        for dep in deps {
            if !state.indices.contains_key(dep) {
                // Case A: dep not yet visited — recurse.
                strongconnect(dep, graph, state);
                let dep_lowlink = state.lowlinks[dep];
                let node_lowlink = state.lowlinks.get_mut(node).unwrap();
                *node_lowlink = (*node_lowlink).min(dep_lowlink);
            } else if state.on_stack.contains(dep) {
                // Case B: dep is on the stack — back edge.
                let dep_index = state.indices[dep];
                let node_lowlink = state.lowlinks.get_mut(node).unwrap();
                *node_lowlink = (*node_lowlink).min(dep_index);
            }
        }
    }

    // If node is a root of an SCC, pop the stack to build the component.
    if state.lowlinks[node] == state.indices[node] {
        let mut component = Vec::new();
        loop {
            let w = state.stack.pop().unwrap();
            state.on_stack.remove(&w);
            let is_root = w == *node;
            component.push(w);
            if is_root {
                break;
            }
        }
        state.components.push(component);
    }
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

    /// Helper: build a `FileDependencyGraph` from `(node, &[dep])` pairs.
    fn make_graph(edges: &[(&str, &[&str])]) -> FileDependencyGraph {
        let mut graph = FileDependencyGraph::new();
        for (node, deps) in edges {
            let dep_set: BTreeSet<PathBuf> = deps.iter().map(|d| PathBuf::from(d)).collect();
            graph.insert(PathBuf::from(node), dep_set);
        }
        graph
    }

    // Test 1: acyclic chain — no cycles.
    #[test]
    fn acyclic_chain() {
        let graph = make_graph(&[
            ("a.py", &["b.py"]),
            ("b.py", &["c.py"]),
            ("c.py", &[]),
        ]);

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
        assert_eq!(
            sccs[2],
            vec![PathBuf::from("y.py"), PathBuf::from("z.py")]
        );

        let cycles = dependency_cycles(&graph);
        assert_eq!(cycles.len(), 2);
        assert_eq!(cycles[0], vec![PathBuf::from("m.py")]);
        assert_eq!(
            cycles[1],
            vec![PathBuf::from("y.py"), PathBuf::from("z.py")]
        );
    }
}
