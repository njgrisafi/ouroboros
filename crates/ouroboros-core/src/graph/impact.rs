use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use super::FileDependencyGraph;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathKind {
    File,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMatch {
    pub kind: PathKind,
    pub nodes: Vec<PathBuf>,
}

/// Match an already-normalized path against the graph node set.
///
/// Exact node match returns a file match. Otherwise, a path-boundary prefix match
/// returns all contained nodes as a directory match.
pub fn match_path(node_paths: &BTreeSet<PathBuf>, path: &Path) -> Option<PathMatch> {
    if node_paths.contains(path) {
        return Some(PathMatch {
            kind: PathKind::File,
            nodes: vec![path.to_path_buf()],
        });
    }

    let dir_prefix = format!("{}/", path.to_string_lossy());
    let nodes: Vec<PathBuf> = node_paths
        .iter()
        .filter(|node| node.to_string_lossy().starts_with(&dir_prefix))
        .cloned()
        .collect();

    if nodes.is_empty() {
        None
    } else {
        Some(PathMatch {
            kind: PathKind::Directory,
            nodes,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Condensation {
    pub scc_members: Vec<Vec<PathBuf>>,
    pub node_to_scc: HashMap<PathBuf, usize>,
    pub scc_adjacency: HashMap<usize, BTreeSet<usize>>,
}

pub fn condensation(graph: &FileDependencyGraph, sccs: &[Vec<PathBuf>]) -> Condensation {
    let mut node_to_scc = HashMap::new();
    for (scc_id, members) in sccs.iter().enumerate() {
        for member in members {
            node_to_scc.insert(member.clone(), scc_id);
        }
    }

    let mut scc_adjacency: HashMap<usize, BTreeSet<usize>> = (0..sccs.len())
        .map(|scc_id| (scc_id, BTreeSet::new()))
        .collect();

    for (from, deps) in graph {
        let Some(&from_scc) = node_to_scc.get(from) else {
            continue;
        };

        for dep in deps {
            let Some(&to_scc) = node_to_scc.get(dep) else {
                continue;
            };

            if from_scc != to_scc {
                scc_adjacency.entry(from_scc).or_default().insert(to_scc);
            }
        }
    }

    Condensation {
        scc_members: sccs.to_vec(),
        node_to_scc,
        scc_adjacency,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReachableCycle {
    pub scc_id: usize,
    pub entry: PathBuf,
    pub path: Vec<PathBuf>,
    pub is_direct: bool,
}

/// Iterative BFS forward reachability from `start`; returns every cycle-SCC reachable
/// with a shortest representative path. Deterministic. O(V + E).
pub fn reachable_cycles_from(
    graph: &FileDependencyGraph,
    start: &PathBuf,
    node_to_scc: &HashMap<PathBuf, usize>,
    cycle_sccs: &HashSet<usize>,
) -> Vec<ReachableCycle> {
    reachable_cycles_from_with_pruning(graph, start, node_to_scc, cycle_sccs, None)
}

pub fn nodes_reaching_cycles(
    graph: &FileDependencyGraph,
    node_to_scc: &HashMap<PathBuf, usize>,
    cycle_sccs: &HashSet<usize>,
) -> HashSet<PathBuf> {
    let mut reverse_graph: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for (from, neighbors) in graph {
        reverse_graph.entry(from.clone()).or_default();
        for neighbor in neighbors {
            reverse_graph
                .entry(neighbor.clone())
                .or_default()
                .push(from.clone());
        }
    }

    let mut reachable = HashSet::new();
    let mut queue = VecDeque::new();
    let mut cycle_nodes: Vec<PathBuf> = node_to_scc
        .iter()
        .filter_map(|(node, scc_id)| cycle_sccs.contains(scc_id).then_some(node.clone()))
        .collect();
    cycle_nodes.sort();

    for node in cycle_nodes {
        if reachable.insert(node.clone()) {
            queue.push_back(node);
        }
    }

    while let Some(node) = queue.pop_front() {
        if let Some(predecessors) = reverse_graph.get(&node) {
            for predecessor in predecessors {
                if reachable.insert(predecessor.clone()) {
                    queue.push_back(predecessor.clone());
                }
            }
        }
    }

    reachable
}

pub fn reachable_cycles_from_pruned(
    graph: &FileDependencyGraph,
    start: &PathBuf,
    node_to_scc: &HashMap<PathBuf, usize>,
    cycle_sccs: &HashSet<usize>,
    nodes_reaching_cycles: &HashSet<PathBuf>,
) -> Vec<ReachableCycle> {
    if !nodes_reaching_cycles.contains(start) {
        return Vec::new();
    }

    reachable_cycles_from_with_pruning(
        graph,
        start,
        node_to_scc,
        cycle_sccs,
        Some(nodes_reaching_cycles),
    )
}

fn reachable_cycles_from_with_pruning(
    graph: &FileDependencyGraph,
    start: &PathBuf,
    node_to_scc: &HashMap<PathBuf, usize>,
    cycle_sccs: &HashSet<usize>,
    nodes_reaching_cycles: Option<&HashSet<PathBuf>>,
) -> Vec<ReachableCycle> {
    let mut dist: HashMap<PathBuf, usize> = HashMap::new();
    let mut pred: HashMap<PathBuf, PathBuf> = HashMap::new();
    let mut queue = VecDeque::new();

    dist.insert(start.clone(), 0);
    queue.push_back(start.clone());

    while let Some(node) = queue.pop_front() {
        let next_dist = dist[&node] + 1;

        if let Some(neighbors) = graph.get(&node) {
            for neighbor in neighbors {
                if nodes_reaching_cycles.is_some_and(|reachable| !reachable.contains(neighbor)) {
                    continue;
                }
                if !dist.contains_key(neighbor) {
                    dist.insert(neighbor.clone(), next_dist);
                    pred.insert(neighbor.clone(), node.clone());
                    queue.push_back(neighbor.clone());
                }
            }
        }
    }

    let mut best_by_scc: HashMap<usize, (PathBuf, Vec<PathBuf>, usize)> = HashMap::new();
    let mut visited_nodes: Vec<PathBuf> = dist
        .keys()
        .filter(|node| {
            node_to_scc
                .get(*node)
                .is_some_and(|scc_id| cycle_sccs.contains(scc_id))
        })
        .cloned()
        .collect();
    visited_nodes.sort();

    for node in visited_nodes {
        let Some(&scc_id) = node_to_scc.get(&node) else {
            continue;
        };

        let candidate_dist = dist[&node];
        if best_by_scc
            .get(&scc_id)
            .is_some_and(|(_, _, best_dist)| candidate_dist > *best_dist)
        {
            continue;
        }

        let path = reconstruct_path(start, &node, &pred);
        let replace = best_by_scc
            .get(&scc_id)
            .map(|(_, best_path, best_dist)| {
                candidate_dist < *best_dist || (candidate_dist == *best_dist && path < *best_path)
            })
            .unwrap_or(true);

        if replace {
            best_by_scc.insert(scc_id, (node, path, candidate_dist));
        }
    }

    let mut cycles: Vec<ReachableCycle> = best_by_scc
        .into_iter()
        .map(|(scc_id, (entry, path, _))| ReachableCycle {
            scc_id,
            entry,
            path,
            is_direct: node_to_scc.get(start) == Some(&scc_id),
        })
        .collect();
    cycles.sort_by_key(|cycle| cycle.scc_id);
    cycles
}

fn reconstruct_path(
    start: &PathBuf,
    target: &PathBuf,
    pred: &HashMap<PathBuf, PathBuf>,
) -> Vec<PathBuf> {
    let mut path = vec![target.clone()];
    let mut current = target;

    while current != start {
        let Some(previous) = pred.get(current) else {
            break;
        };
        path.push(previous.clone());
        current = previous;
    }

    path.reverse();
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(value: &str) -> PathBuf {
        PathBuf::from(value)
    }

    fn paths(values: &[&str]) -> Vec<PathBuf> {
        values.iter().map(|value| path(value)).collect()
    }

    fn node_set(values: &[&str]) -> BTreeSet<PathBuf> {
        values.iter().map(|value| path(value)).collect()
    }

    fn make_graph(edges: &[(&str, &[&str])]) -> FileDependencyGraph {
        let mut graph = FileDependencyGraph::new();
        for (node, deps) in edges {
            graph.insert(path(node), deps.iter().map(|dep| path(dep)).collect());
        }
        graph
    }

    fn cycle_ids(ids: &[usize]) -> HashSet<usize> {
        ids.iter().copied().collect()
    }

    #[test]
    fn match_path_exact_file_match() {
        let nodes = node_set(&["app/main.py", "app/util.py"]);

        let matched = match_path(&nodes, Path::new("app/main.py")).unwrap();

        assert_eq!(matched.kind, PathKind::File);
        assert_eq!(matched.nodes, paths(&["app/main.py"]));
    }

    #[test]
    fn match_path_directory_prefix_sorted() {
        let nodes = node_set(&["app/z.py", "other.py", "app/a.py", "app/pkg/m.py"]);

        let matched = match_path(&nodes, Path::new("app")).unwrap();

        assert_eq!(matched.kind, PathKind::Directory);
        assert_eq!(
            matched.nodes,
            paths(&["app/a.py", "app/pkg/m.py", "app/z.py"])
        );
    }

    #[test]
    fn match_path_no_match_returns_none() {
        let nodes = node_set(&["app/main.py"]);

        assert!(match_path(&nodes, Path::new("missing")).is_none());
    }

    #[test]
    fn match_path_prefix_boundary_does_not_match_similar_name() {
        let nodes = node_set(&["app_x/foo.py"]);

        assert!(match_path(&nodes, Path::new("app")).is_none());
    }

    #[test]
    fn match_path_empty_node_set_returns_none() {
        assert!(match_path(&BTreeSet::new(), Path::new("app")).is_none());
    }

    #[test]
    fn condensation_node_to_scc_coverage() {
        let graph = make_graph(&[("a.py", &["b.py"]), ("b.py", &[]), ("c.py", &[])]);
        let sccs = vec![paths(&["a.py"]), paths(&["b.py"]), paths(&["c.py"])];

        let condensed = condensation(&graph, &sccs);

        assert_eq!(condensed.node_to_scc[&path("a.py")], 0);
        assert_eq!(condensed.node_to_scc[&path("b.py")], 1);
        assert_eq!(condensed.node_to_scc[&path("c.py")], 2);
    }

    #[test]
    fn condensation_dag_excludes_self_loops() {
        let graph = make_graph(&[
            ("a.py", &["b.py"]),
            ("b.py", &["a.py", "c.py"]),
            ("c.py", &[]),
        ]);
        let sccs = vec![paths(&["a.py", "b.py"]), paths(&["c.py"])];

        let condensed = condensation(&graph, &sccs);

        let first_edges = &condensed.scc_adjacency[&0];

        assert!(!first_edges.contains(&0));
        assert!(first_edges.contains(&1));
    }

    #[test]
    fn condensation_multi_node_scc_collapse() {
        let graph = make_graph(&[("a.py", &["b.py"]), ("b.py", &["a.py"])]);
        let sccs = vec![paths(&["a.py", "b.py"])];

        let condensed = condensation(&graph, &sccs);

        assert_eq!(condensed.node_to_scc[&path("a.py")], 0);
        assert_eq!(condensed.node_to_scc[&path("b.py")], 0);
        assert!(condensed.scc_adjacency[&0].is_empty());
    }

    #[test]
    fn reachable_cycles_direct_member() {
        let graph = make_graph(&[("a.py", &["b.py"]), ("b.py", &["a.py"])]);
        let condensed = condensation(&graph, &[paths(&["a.py", "b.py"])]);

        let cycles = reachable_cycles_from(
            &graph,
            &path("a.py"),
            &condensed.node_to_scc,
            &cycle_ids(&[0]),
        );

        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].scc_id, 0);
        assert_eq!(cycles[0].entry, path("a.py"));
        assert_eq!(cycles[0].path, paths(&["a.py"]));
        assert!(cycles[0].is_direct);
    }

    #[test]
    fn reachable_cycles_reachable_branch_shortest_path() {
        let graph = make_graph(&[
            ("start.py", &["left.py", "right.py"]),
            ("left.py", &["cycle_a.py"]),
            ("right.py", &["extra.py"]),
            ("extra.py", &["cycle_b.py"]),
            ("cycle_a.py", &["cycle_b.py"]),
            ("cycle_b.py", &["cycle_a.py"]),
        ]);
        let condensed = condensation(
            &graph,
            &[
                paths(&["start.py"]),
                paths(&["left.py"]),
                paths(&["right.py"]),
                paths(&["extra.py"]),
                paths(&["cycle_a.py", "cycle_b.py"]),
            ],
        );

        let cycles = reachable_cycles_from(
            &graph,
            &path("start.py"),
            &condensed.node_to_scc,
            &cycle_ids(&[4]),
        );

        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].entry, path("cycle_a.py"));
        assert_eq!(
            cycles[0].path,
            paths(&["start.py", "left.py", "cycle_a.py"])
        );
        assert!(!cycles[0].is_direct);
    }

    #[test]
    fn reachable_cycles_unreachable_empty_result() {
        let graph = make_graph(&[
            ("start.py", &["leaf.py"]),
            ("leaf.py", &[]),
            ("cycle.py", &["cycle.py"]),
        ]);
        let condensed = condensation(
            &graph,
            &[
                paths(&["start.py"]),
                paths(&["leaf.py"]),
                paths(&["cycle.py"]),
            ],
        );

        let cycles = reachable_cycles_from(
            &graph,
            &path("start.py"),
            &condensed.node_to_scc,
            &cycle_ids(&[2]),
        );

        assert!(cycles.is_empty());
    }

    #[test]
    fn pruned_reachability_excludes_clean_branch_and_preserves_results() {
        let graph = make_graph(&[
            ("start.py", &["mid.py", "clean.py"]),
            ("mid.py", &["cycle_a.py"]),
            ("clean.py", &["leaf.py"]),
            ("leaf.py", &[]),
            ("cycle_a.py", &["cycle_b.py"]),
            ("cycle_b.py", &["cycle_a.py"]),
        ]);
        let condensed = condensation(
            &graph,
            &[
                paths(&["start.py"]),
                paths(&["mid.py"]),
                paths(&["clean.py"]),
                paths(&["leaf.py"]),
                paths(&["cycle_a.py", "cycle_b.py"]),
            ],
        );
        let cycle_sccs = cycle_ids(&[4]);
        let reachable_nodes = nodes_reaching_cycles(&graph, &condensed.node_to_scc, &cycle_sccs);

        assert!(reachable_nodes.contains(&path("start.py")));
        assert!(reachable_nodes.contains(&path("mid.py")));
        assert!(!reachable_nodes.contains(&path("clean.py")));
        assert!(!reachable_nodes.contains(&path("leaf.py")));

        let unpruned = reachable_cycles_from(
            &graph,
            &path("start.py"),
            &condensed.node_to_scc,
            &cycle_sccs,
        );
        let pruned = reachable_cycles_from_pruned(
            &graph,
            &path("start.py"),
            &condensed.node_to_scc,
            &cycle_sccs,
            &reachable_nodes,
        );

        assert_eq!(pruned, unpruned);
        assert!(
            reachable_cycles_from_pruned(
                &graph,
                &path("clean.py"),
                &condensed.node_to_scc,
                &cycle_sccs,
                &reachable_nodes,
            )
            .is_empty()
        );
    }

    #[test]
    fn reachable_cycles_multiple_reachable_cycles() {
        let graph = make_graph(&[
            ("start.py", &["first.py", "second.py"]),
            ("first.py", &["a.py"]),
            ("a.py", &["a.py"]),
            ("second.py", &["b.py"]),
            ("b.py", &["c.py"]),
            ("c.py", &["b.py"]),
        ]);
        let condensed = condensation(
            &graph,
            &[
                paths(&["start.py"]),
                paths(&["first.py"]),
                paths(&["a.py"]),
                paths(&["second.py"]),
                paths(&["b.py", "c.py"]),
            ],
        );

        let cycles = reachable_cycles_from(
            &graph,
            &path("start.py"),
            &condensed.node_to_scc,
            &cycle_ids(&[2, 4]),
        );

        assert_eq!(cycles.len(), 2);
        assert_eq!(cycles[0].scc_id, 2);
        assert_eq!(cycles[0].path, paths(&["start.py", "first.py", "a.py"]));
        assert_eq!(cycles[1].scc_id, 4);
        assert_eq!(cycles[1].path, paths(&["start.py", "second.py", "b.py"]));
    }

    #[test]
    fn reachable_cycles_shortest_path_tie_break_is_lexicographic() {
        let graph = make_graph(&[
            ("start.py", &["a.py", "b.py"]),
            ("a.py", &["z_entry.py"]),
            ("b.py", &["m_entry.py"]),
            ("m_entry.py", &["z_entry.py"]),
            ("z_entry.py", &["m_entry.py"]),
        ]);
        let condensed = condensation(
            &graph,
            &[
                paths(&["start.py"]),
                paths(&["a.py"]),
                paths(&["b.py"]),
                paths(&["m_entry.py", "z_entry.py"]),
            ],
        );

        let cycles = reachable_cycles_from(
            &graph,
            &path("start.py"),
            &condensed.node_to_scc,
            &cycle_ids(&[3]),
        );

        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].entry, path("z_entry.py"));
        assert_eq!(cycles[0].path, paths(&["start.py", "a.py", "z_entry.py"]));
    }

    #[test]
    fn reachable_cycles_self_loop_cycle() {
        let graph = make_graph(&[("start.py", &["cycle.py"]), ("cycle.py", &["cycle.py"])]);
        let condensed = condensation(&graph, &[paths(&["start.py"]), paths(&["cycle.py"])]);

        let cycles = reachable_cycles_from(
            &graph,
            &path("start.py"),
            &condensed.node_to_scc,
            &cycle_ids(&[1]),
        );

        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].entry, path("cycle.py"));
        assert_eq!(cycles[0].path, paths(&["start.py", "cycle.py"]));
        assert!(!cycles[0].is_direct);
    }

    #[test]
    fn reachable_cycles_deep_linear_chain_is_iterative() {
        let mut graph = FileDependencyGraph::new();
        for index in 0..100 {
            graph.insert(
                path(&format!("n{index:03}.py")),
                BTreeSet::from([path(&format!("n{:03}.py", index + 1))]),
            );
        }
        graph.insert(path("n100.py"), BTreeSet::from([path("n100.py")]));

        let mut sccs = Vec::new();
        for index in 0..100 {
            sccs.push(vec![path(&format!("n{index:03}.py"))]);
        }
        sccs.push(vec![path("n100.py")]);
        let condensed = condensation(&graph, &sccs);

        let cycles = reachable_cycles_from(
            &graph,
            &path("n000.py"),
            &condensed.node_to_scc,
            &cycle_ids(&[100]),
        );

        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].entry, path("n100.py"));
        assert_eq!(cycles[0].path.len(), 101);
        assert_eq!(cycles[0].path.first(), Some(&path("n000.py")));
        assert_eq!(cycles[0].path.last(), Some(&path("n100.py")));
    }
}
