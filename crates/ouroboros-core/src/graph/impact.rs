use std::collections::{BTreeSet, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use super::FileDependencyGraph;
use super::interned::{InternedGraph, NO_NODE};

/// Build a node-id → SCC-id map (`NO_NODE` when a node is in no listed SCC).
pub fn interned_node_to_scc(igraph: &InternedGraph, sccs: &[Vec<PathBuf>]) -> Vec<u32> {
    let mut node_to_scc = vec![NO_NODE; igraph.len()];
    for (scc_id, members) in sccs.iter().enumerate() {
        for member in members {
            if let Some(id) = igraph.id(member) {
                node_to_scc[id as usize] = scc_id as u32;
            }
        }
    }
    node_to_scc
}

/// Look up the SCC id for a path, or `None` if it is in no listed SCC.
pub fn scc_of_path(igraph: &InternedGraph, node_to_scc: &[u32], path: &PathBuf) -> Option<usize> {
    let id = igraph.id(path)?;
    let scc = node_to_scc[id as usize];
    (scc != NO_NODE).then_some(scc as usize)
}

/// Convert a set of SCC ids to a flag array indexed by SCC id.
pub fn cycle_scc_flags(cycle_sccs: &HashSet<usize>, scc_count: usize) -> Vec<bool> {
    let size = scc_count.max(cycle_sccs.iter().max().map_or(0, |max| max + 1));
    let mut flags = vec![false; size];
    for &scc in cycle_sccs {
        flags[scc] = true;
    }
    flags
}

/// Interned reverse BFS: flag every node that can reach a cycle SCC.
pub fn interned_nodes_reaching_cycles(
    igraph: &InternedGraph,
    node_to_scc: &[u32],
    cycle_scc: &[bool],
) -> Vec<bool> {
    let mut reverse: Vec<Vec<u32>> = vec![Vec::new(); igraph.len()];
    for (from, deps) in igraph.adjacency.iter().enumerate() {
        for &to in deps {
            reverse[to as usize].push(from as u32);
        }
    }

    let mut reaching = vec![false; igraph.len()];
    let mut queue = VecDeque::new();
    for (id, &scc) in node_to_scc.iter().enumerate() {
        if scc != NO_NODE && cycle_scc[scc as usize] && !reaching[id] {
            reaching[id] = true;
            queue.push_back(id as u32);
        }
    }

    while let Some(node) = queue.pop_front() {
        for &predecessor in &reverse[node as usize] {
            if !reaching[predecessor as usize] {
                reaching[predecessor as usize] = true;
                queue.push_back(predecessor);
            }
        }
    }

    reaching
}

/// Interned forward BFS from `start`, collecting the best (shortest, then
/// lexicographically smallest) representative path to each reachable cycle
/// SCC. `reaching`, when given, prunes the search to nodes that can reach a
/// cycle. Ids iterate in lexicographic path order, so results are identical
/// to the `PathBuf`-keyed algorithm.
pub fn interned_reachable_cycles(
    igraph: &InternedGraph,
    start: u32,
    node_to_scc: &[u32],
    cycle_scc: &[bool],
    reaching: Option<&[bool]>,
) -> Vec<ReachableCycle> {
    if reaching.is_some_and(|flags| !flags[start as usize]) {
        return Vec::new();
    }

    let node_count = igraph.len();
    let mut dist = vec![NO_NODE; node_count];
    let mut pred = vec![NO_NODE; node_count];
    let mut queue = VecDeque::new();

    dist[start as usize] = 0;
    queue.push_back(start);

    while let Some(node) = queue.pop_front() {
        let next_dist = dist[node as usize] + 1;
        for &neighbor in &igraph.adjacency[node as usize] {
            if reaching.is_some_and(|flags| !flags[neighbor as usize]) {
                continue;
            }
            if dist[neighbor as usize] == NO_NODE {
                dist[neighbor as usize] = next_dist;
                pred[neighbor as usize] = node;
                queue.push_back(neighbor);
            }
        }
    }

    // Best (entry, path, dist) per cycle SCC. Iterating ids 0..n visits nodes
    // in lexicographic path order, matching the sorted iteration of the
    // PathBuf-keyed version.
    let mut best: Vec<Option<(u32, Vec<u32>, u32)>> = vec![None; cycle_scc.len()];
    for node in 0..node_count as u32 {
        if dist[node as usize] == NO_NODE {
            continue;
        }
        let scc = node_to_scc[node as usize];
        if scc == NO_NODE || !cycle_scc[scc as usize] {
            continue;
        }

        let candidate_dist = dist[node as usize];
        // Cheap guard: skip path reconstruction when the candidate is
        // strictly worse than the current best.
        if best[scc as usize]
            .as_ref()
            .is_some_and(|(_, _, best_dist)| candidate_dist > *best_dist)
        {
            continue;
        }

        let path = reconstruct_path_ids(start, node, &pred);
        let replace = best[scc as usize]
            .as_ref()
            .is_none_or(|(_, best_path, best_dist)| {
                is_better_path(candidate_dist, &path, *best_dist, best_path)
            });
        if replace {
            best[scc as usize] = Some((node, path, candidate_dist));
        }
    }

    let mut cycles: Vec<ReachableCycle> = best
        .into_iter()
        .enumerate()
        .filter_map(|(scc_id, slot)| {
            let (entry, path, _) = slot?;
            Some(ReachableCycle {
                scc_id,
                entry: igraph.path(entry).clone(),
                path: path.into_iter().map(|id| igraph.path(id).clone()).collect(),
                is_direct: node_to_scc[start as usize] == scc_id as u32,
            })
        })
        .collect();
    cycles.sort_by_key(|cycle| cycle.scc_id);
    cycles
}

/// Whether a candidate (dist, path) beats the current best: shorter wins,
/// ties break lexicographically. Id order is lexicographic path order, so
/// comparing id paths is equivalent to comparing path strings.
fn is_better_path(
    candidate_dist: u32,
    candidate_path: &[u32],
    best_dist: u32,
    best_path: &[u32],
) -> bool {
    candidate_dist < best_dist || (candidate_dist == best_dist && candidate_path < best_path)
}

fn reconstruct_path_ids(start: u32, target: u32, pred: &[u32]) -> Vec<u32> {
    let mut path = vec![target];
    let mut current = target;

    while current != start {
        let previous = pred[current as usize];
        if previous == NO_NODE {
            break;
        }
        path.push(previous);
        current = previous;
    }

    path.reverse();
    path
}

/// Interned forward reachability from a set of start ids.
fn interned_reachable_from(igraph: &InternedGraph, starts: impl Iterator<Item = u32>) -> Vec<bool> {
    let mut visited = vec![false; igraph.len()];
    let mut queue = VecDeque::new();

    for start in starts {
        if !visited[start as usize] {
            visited[start as usize] = true;
            queue.push_back(start);
        }
    }

    while let Some(node) = queue.pop_front() {
        for &neighbor in &igraph.adjacency[node as usize] {
            if !visited[neighbor as usize] {
                visited[neighbor as usize] = true;
                queue.push_back(neighbor);
            }
        }
    }

    visited
}

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
pub struct ReachableCycle {
    pub scc_id: usize,
    pub entry: PathBuf,
    pub path: Vec<PathBuf>,
    pub is_direct: bool,
}

/// Forward BFS from a set of starting nodes.
///
/// Returns every node reachable from any start by following forward import edges
/// (inclusive of the starts themselves). Terminates on cycles (visited set guards
/// re-enqueue). O(V + E).
pub fn reachable_from(graph: &FileDependencyGraph, starts: &HashSet<PathBuf>) -> HashSet<PathBuf> {
    let igraph = InternedGraph::new(graph);
    let visited = interned_reachable_from(&igraph, starts.iter().filter_map(|s| igraph.id(s)));
    visited
        .iter()
        .enumerate()
        .filter(|&(_, &flag)| flag)
        .map(|(id, _)| igraph.path(id as u32).clone())
        .collect()
}

/// Prune the graph to the forward-reachable induced subgraph from non-excluded seeds.
///
/// Seeds are all graph nodes NOT in `excluded`. The retained set R is every node
/// reachable from any seed by following forward import edges (inclusive of seeds).
/// Returns the induced subgraph on R: only nodes in R, with edges filtered to R×R.
///
/// If `excluded` is empty, returns a graph equal to the input (all nodes are seeds).
/// Excluded nodes that are reachable from a seed (e.g. because a non-excluded file
/// imports them) are retained — this is the mypy-faithful semantics.
pub fn apply_exclusions(
    graph: &FileDependencyGraph,
    excluded: &HashSet<PathBuf>,
) -> FileDependencyGraph {
    let seeds: HashSet<PathBuf> = graph
        .keys()
        .filter(|n| !excluded.contains(*n))
        .cloned()
        .collect();

    let retained = reachable_from(graph, &seeds);

    graph
        .iter()
        .filter(|(node, _)| retained.contains(*node))
        .map(|(node, deps)| {
            let filtered_deps: BTreeSet<PathBuf> = deps
                .iter()
                .filter(|d| retained.contains(*d))
                .cloned()
                .collect();
            (node.clone(), filtered_deps)
        })
        .collect()
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
        let mut graph = FileDependencyGraph::default();
        for (node, deps) in edges {
            graph.insert(path(node), deps.iter().map(|dep| path(dep)).collect());
        }
        graph
    }

    fn cycle_ids(ids: &[usize]) -> HashSet<usize> {
        ids.iter().copied().collect()
    }

    /// An interned graph plus its node→SCC map, mirroring how the CLI's
    /// trace analysis sets up the interned queries.
    struct TraceFixture<'a> {
        igraph: InternedGraph<'a>,
        node_to_scc: Vec<u32>,
        scc_count: usize,
    }

    impl<'a> TraceFixture<'a> {
        fn new(graph: &'a FileDependencyGraph, sccs: &[Vec<PathBuf>]) -> Self {
            let igraph = InternedGraph::new(graph);
            let node_to_scc = interned_node_to_scc(&igraph, sccs);
            TraceFixture {
                igraph,
                node_to_scc,
                scc_count: sccs.len(),
            }
        }

        fn flags(&self, cycle_sccs: &HashSet<usize>) -> Vec<bool> {
            cycle_scc_flags(cycle_sccs, self.scc_count)
        }

        fn start_id(&self, start: &str) -> u32 {
            self.igraph.id(&path(start)).expect("start node")
        }

        fn cycles_from(&self, start: &str, cycle_sccs: &HashSet<usize>) -> Vec<ReachableCycle> {
            let flags = self.flags(cycle_sccs);
            interned_reachable_cycles(
                &self.igraph,
                self.start_id(start),
                &self.node_to_scc,
                &flags,
                None,
            )
        }
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
    fn node_to_scc_coverage() {
        let graph = make_graph(&[("a.py", &["b.py"]), ("b.py", &[]), ("c.py", &[])]);
        let sccs = vec![paths(&["a.py"]), paths(&["b.py"]), paths(&["c.py"])];

        let fixture = TraceFixture::new(&graph, &sccs);

        assert_eq!(
            scc_of_path(&fixture.igraph, &fixture.node_to_scc, &path("a.py")),
            Some(0)
        );
        assert_eq!(
            scc_of_path(&fixture.igraph, &fixture.node_to_scc, &path("b.py")),
            Some(1)
        );
        assert_eq!(
            scc_of_path(&fixture.igraph, &fixture.node_to_scc, &path("c.py")),
            Some(2)
        );
    }

    #[test]
    fn node_to_scc_multi_node_scc_collapse() {
        let graph = make_graph(&[("a.py", &["b.py"]), ("b.py", &["a.py"])]);
        let sccs = vec![paths(&["a.py", "b.py"])];

        let fixture = TraceFixture::new(&graph, &sccs);

        assert_eq!(
            scc_of_path(&fixture.igraph, &fixture.node_to_scc, &path("a.py")),
            Some(0)
        );
        assert_eq!(
            scc_of_path(&fixture.igraph, &fixture.node_to_scc, &path("b.py")),
            Some(0)
        );
    }

    #[test]
    fn node_to_scc_missing_node_is_none() {
        let graph = make_graph(&[("a.py", &[])]);
        let sccs = vec![paths(&["a.py"])];

        let fixture = TraceFixture::new(&graph, &sccs);

        assert_eq!(
            scc_of_path(&fixture.igraph, &fixture.node_to_scc, &path("ghost.py")),
            None
        );
    }

    #[test]
    fn reachable_cycles_direct_member() {
        let graph = make_graph(&[("a.py", &["b.py"]), ("b.py", &["a.py"])]);
        let fixture = TraceFixture::new(&graph, &[paths(&["a.py", "b.py"])]);

        let cycles = fixture.cycles_from("a.py", &cycle_ids(&[0]));

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
        let fixture = TraceFixture::new(
            &graph,
            &[
                paths(&["start.py"]),
                paths(&["left.py"]),
                paths(&["right.py"]),
                paths(&["extra.py"]),
                paths(&["cycle_a.py", "cycle_b.py"]),
            ],
        );

        let cycles = fixture.cycles_from("start.py", &cycle_ids(&[4]));

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
        let fixture = TraceFixture::new(
            &graph,
            &[
                paths(&["start.py"]),
                paths(&["leaf.py"]),
                paths(&["cycle.py"]),
            ],
        );

        let cycles = fixture.cycles_from("start.py", &cycle_ids(&[2]));

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
        let fixture = TraceFixture::new(
            &graph,
            &[
                paths(&["start.py"]),
                paths(&["mid.py"]),
                paths(&["clean.py"]),
                paths(&["leaf.py"]),
                paths(&["cycle_a.py", "cycle_b.py"]),
            ],
        );
        let flags = fixture.flags(&cycle_ids(&[4]));
        let reaching =
            interned_nodes_reaching_cycles(&fixture.igraph, &fixture.node_to_scc, &flags);

        let reaches = |name: &str| reaching[fixture.start_id(name) as usize];
        assert!(reaches("start.py"));
        assert!(reaches("mid.py"));
        assert!(!reaches("clean.py"));
        assert!(!reaches("leaf.py"));

        let start = fixture.start_id("start.py");
        let unpruned =
            interned_reachable_cycles(&fixture.igraph, start, &fixture.node_to_scc, &flags, None);
        let pruned = interned_reachable_cycles(
            &fixture.igraph,
            start,
            &fixture.node_to_scc,
            &flags,
            Some(&reaching),
        );

        assert_eq!(pruned, unpruned);
        assert!(
            interned_reachable_cycles(
                &fixture.igraph,
                fixture.start_id("clean.py"),
                &fixture.node_to_scc,
                &flags,
                Some(&reaching),
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
        let fixture = TraceFixture::new(
            &graph,
            &[
                paths(&["start.py"]),
                paths(&["first.py"]),
                paths(&["a.py"]),
                paths(&["second.py"]),
                paths(&["b.py", "c.py"]),
            ],
        );

        let cycles = fixture.cycles_from("start.py", &cycle_ids(&[2, 4]));

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
        let fixture = TraceFixture::new(
            &graph,
            &[
                paths(&["start.py"]),
                paths(&["a.py"]),
                paths(&["b.py"]),
                paths(&["m_entry.py", "z_entry.py"]),
            ],
        );

        let cycles = fixture.cycles_from("start.py", &cycle_ids(&[3]));

        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].entry, path("z_entry.py"));
        assert_eq!(cycles[0].path, paths(&["start.py", "a.py", "z_entry.py"]));
    }

    #[test]
    fn reachable_cycles_self_loop_cycle() {
        let graph = make_graph(&[("start.py", &["cycle.py"]), ("cycle.py", &["cycle.py"])]);
        let fixture = TraceFixture::new(&graph, &[paths(&["start.py"]), paths(&["cycle.py"])]);

        let cycles = fixture.cycles_from("start.py", &cycle_ids(&[1]));

        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].entry, path("cycle.py"));
        assert_eq!(cycles[0].path, paths(&["start.py", "cycle.py"]));
        assert!(!cycles[0].is_direct);
    }

    #[test]
    fn reachable_cycles_deep_linear_chain_is_iterative() {
        let mut graph = FileDependencyGraph::default();
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
        let fixture = TraceFixture::new(&graph, &sccs);

        let cycles = fixture.cycles_from("n000.py", &cycle_ids(&[100]));

        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].entry, path("n100.py"));
        assert_eq!(cycles[0].path.len(), 101);
        assert_eq!(cycles[0].path.first(), Some(&path("n000.py")));
        assert_eq!(cycles[0].path.last(), Some(&path("n100.py")));
    }

    #[test]
    fn reachable_from_linear_chain() {
        let graph = make_graph(&[("a.py", &["b.py"]), ("b.py", &["c.py"]), ("c.py", &[])]);
        let starts: HashSet<PathBuf> = [path("a.py")].into_iter().collect();
        let result = reachable_from(&graph, &starts);
        assert!(result.contains(&path("a.py")));
        assert!(result.contains(&path("b.py")));
        assert!(result.contains(&path("c.py")));
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn reachable_from_self_loop_terminates() {
        let graph = make_graph(&[("a.py", &["a.py"])]);
        let starts: HashSet<PathBuf> = [path("a.py")].into_iter().collect();
        let result = reachable_from(&graph, &starts);
        assert_eq!(result, [path("a.py")].into_iter().collect());
    }

    #[test]
    fn reachable_from_disconnected_node_excluded() {
        let graph = make_graph(&[("a.py", &["b.py"]), ("b.py", &[]), ("x.py", &[])]);
        let starts: HashSet<PathBuf> = [path("a.py")].into_iter().collect();
        let result = reachable_from(&graph, &starts);
        assert!(result.contains(&path("a.py")));
        assert!(result.contains(&path("b.py")));
        assert!(!result.contains(&path("x.py")));
    }

    #[test]
    fn reachable_from_direction_forward_only() {
        let graph = make_graph(&[("a.py", &["b.py"]), ("b.py", &[]), ("d.py", &["a.py"])]);
        let starts: HashSet<PathBuf> = [path("a.py")].into_iter().collect();
        let result = reachable_from(&graph, &starts);
        assert!(result.contains(&path("a.py")));
        assert!(result.contains(&path("b.py")));
        assert!(
            !result.contains(&path("d.py")),
            "d imports a but a does not import d; d must not be reachable"
        );
    }

    #[test]
    fn reachable_from_multiple_starts_union() {
        let graph = make_graph(&[
            ("a.py", &["b.py"]),
            ("b.py", &[]),
            ("c.py", &["d.py"]),
            ("d.py", &[]),
        ]);
        let starts: HashSet<PathBuf> = [path("a.py"), path("c.py")].into_iter().collect();
        let result = reachable_from(&graph, &starts);
        assert_eq!(result.len(), 4);
    }

    #[test]
    fn reachable_from_empty_starts() {
        let graph = make_graph(&[("a.py", &["b.py"]), ("b.py", &[])]);
        let starts: HashSet<PathBuf> = HashSet::new();
        let result = reachable_from(&graph, &starts);
        assert!(result.is_empty());
    }

    #[test]
    fn apply_exclusions_mutual_cycle_one_excluded_both_retained() {
        let graph = make_graph(&[("a.py", &["b.py"]), ("b.py", &["a.py"])]);
        let excluded: HashSet<PathBuf> = [path("b.py")].into_iter().collect();
        let result = apply_exclusions(&graph, &excluded);
        assert!(result.contains_key(&path("a.py")));
        assert!(result.contains_key(&path("b.py")));
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn apply_exclusions_fully_excluded_cycle_pruned() {
        let graph = make_graph(&[("x.py", &["y.py"]), ("y.py", &["x.py"])]);
        let excluded: HashSet<PathBuf> = [path("x.py"), path("y.py")].into_iter().collect();
        let result = apply_exclusions(&graph, &excluded);
        assert!(result.is_empty());
    }

    #[test]
    fn apply_exclusions_importer_only_excluded_dropped() {
        let graph = make_graph(&[("t.py", &["a.py"]), ("a.py", &[])]);
        let excluded: HashSet<PathBuf> = [path("t.py")].into_iter().collect();
        let result = apply_exclusions(&graph, &excluded);
        assert!(result.contains_key(&path("a.py")));
        assert!(!result.contains_key(&path("t.py")));
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn apply_exclusions_chain_excluded_unreachable_dropped() {
        let graph = make_graph(&[
            ("ex1.py", &["ex2.py"]),
            ("ex2.py", &["app.py"]),
            ("app.py", &[]),
        ]);
        let excluded: HashSet<PathBuf> = [path("ex1.py"), path("ex2.py")].into_iter().collect();
        let result = apply_exclusions(&graph, &excluded);
        assert!(result.contains_key(&path("app.py")));
        assert!(!result.contains_key(&path("ex1.py")));
        assert!(!result.contains_key(&path("ex2.py")));
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn apply_exclusions_empty_excluded_returns_identical_graph() {
        let graph = make_graph(&[("a.py", &["b.py"]), ("b.py", &["c.py"]), ("c.py", &[])]);
        let excluded: HashSet<PathBuf> = HashSet::new();
        let result = apply_exclusions(&graph, &excluded);
        assert_eq!(result.len(), graph.len());
        for (node, deps) in &graph {
            assert_eq!(result.get(node), Some(deps));
        }
    }

    #[test]
    fn apply_exclusions_self_loop_reachable_retained() {
        let graph = make_graph(&[("a.py", &["s.py"]), ("s.py", &["s.py"])]);
        let excluded: HashSet<PathBuf> = [path("s.py")].into_iter().collect();
        let result = apply_exclusions(&graph, &excluded);
        assert!(result.contains_key(&path("a.py")));
        assert!(result.contains_key(&path("s.py")));
        assert!(result[&path("s.py")].contains(&path("s.py")));
    }
}
