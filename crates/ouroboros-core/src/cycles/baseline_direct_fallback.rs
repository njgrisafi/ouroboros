use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::graph::{
    FileCycle, FileDependencyGraph, cycles_from_sccs, induced_subgraph_on,
    strongly_connected_components,
};

/// Collect cyclic files for the `ignore-derived-ancestor-init` baseline.
///
/// For each cycle in the normal report:
/// 1. If the direct-only subgraph (ancestor-init edges stripped) has a cyclic SCC
///    within those members, keep only those direct-only members (preserves existing baseline).
/// 2. Otherwise the cycle was wholly absent from a direct-only baseline because derived
///    ancestor-init edges close the loop — include all SCC members from the normal report.
pub fn collect_cyclic_files_with_direct_fallback(
    full_cycles: &[FileCycle],
    direct_graph: &FileDependencyGraph,
) -> Vec<PathBuf> {
    let mut included = BTreeSet::new();

    for cycle in full_cycles {
        let members: BTreeSet<PathBuf> = cycle.iter().cloned().collect();
        let direct_members = files_from_direct_sccs(&members, direct_graph);
        if direct_members.is_empty() {
            included.extend(members);
        } else {
            included.extend(direct_members);
        }
    }

    included.into_iter().collect()
}

fn files_from_direct_sccs(
    members: &BTreeSet<PathBuf>,
    direct_graph: &FileDependencyGraph,
) -> BTreeSet<PathBuf> {
    let subgraph = induced_subgraph_on(direct_graph, members);
    let sccs = strongly_connected_components(&subgraph);
    cycles_from_sccs(&subgraph, &sccs)
        .into_iter()
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn pb(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    fn edge(graph: &mut FileDependencyGraph, from: &str, to: &str) {
        graph.entry(pb(from)).or_default().insert(pb(to));
    }

    fn cycle(members: &[&str]) -> FileCycle {
        members.iter().map(|m| pb(m)).collect()
    }

    #[test]
    fn empty_cycles_returns_empty() {
        let direct = FileDependencyGraph::default();
        assert!(collect_cyclic_files_with_direct_fallback(&[], &direct).is_empty());
    }

    #[test]
    fn direct_init_cycle_includes_both_members() {
        let mut direct = FileDependencyGraph::default();
        edge(&mut direct, "src/pkg/__init__.py", "src/pkg/mod.py");
        edge(&mut direct, "src/pkg/mod.py", "src/pkg/__init__.py");

        let cycles = vec![cycle(&["src/pkg/__init__.py", "src/pkg/mod.py"])];
        let result = collect_cyclic_files_with_direct_fallback(&cycles, &direct);

        assert_eq!(
            result,
            vec![pb("src/pkg/__init__.py"), pb("src/pkg/mod.py")]
        );
    }

    #[test]
    fn untracked_cycle_includes_all_members() {
        let mut direct = FileDependencyGraph::default();
        edge(&mut direct, "src/alpha/__init__.py", "src/beta/helpers.py");
        edge(&mut direct, "src/beta/helpers.py", "src/alpha/core.py");

        let cycles = vec![cycle(&["src/alpha/__init__.py", "src/beta/helpers.py"])];
        let result = collect_cyclic_files_with_direct_fallback(&cycles, &direct);

        assert_eq!(
            result,
            vec![pb("src/alpha/__init__.py"), pb("src/beta/helpers.py")]
        );
    }

    #[test]
    fn untracked_mixed_derived_cycle_includes_all_members() {
        let mut direct = FileDependencyGraph::default();

        edge(
            &mut direct,
            "src/provider/__init__.py",
            "src/provider/factory.py",
        );
        edge(
            &mut direct,
            "src/provider/factory.py",
            "src/provider/inplace/__init__.py",
        );
        edge(
            &mut direct,
            "src/provider/inplace/__init__.py",
            "src/provider/inplace/hierarchy_source.py",
        );
        edge(
            &mut direct,
            "src/provider/inplace/hierarchy_source.py",
            "src/tree/inplace_contract.py",
        );
        edge(
            &mut direct,
            "src/tree/inplace_contract.py",
            "src/provider/inplace/accessor/base.py",
        );

        let cycles = vec![cycle(&[
            "src/provider/__init__.py",
            "src/provider/factory.py",
            "src/provider/inplace/__init__.py",
            "src/provider/inplace/hierarchy_source.py",
            "src/tree/inplace_contract.py",
        ])];
        let result = collect_cyclic_files_with_direct_fallback(&cycles, &direct);

        assert_eq!(
            result,
            vec![
                pb("src/provider/__init__.py"),
                pb("src/provider/factory.py"),
                pb("src/provider/inplace/__init__.py"),
                pb("src/provider/inplace/hierarchy_source.py"),
                pb("src/tree/inplace_contract.py"),
            ]
        );
    }

    #[test]
    fn partially_tracked_cycle_does_not_expand() {
        let mut direct = FileDependencyGraph::default();
        edge(&mut direct, "a.py", "b.py");
        edge(&mut direct, "b.py", "a.py");
        edge(&mut direct, "b.py", "c.py");

        let cycles = vec![cycle(&["a.py", "b.py", "c.py"])];
        let result = collect_cyclic_files_with_direct_fallback(&cycles, &direct);

        assert_eq!(result, vec![pb("a.py"), pb("b.py")]);
        assert!(
            !result.contains(&pb("c.py")),
            "already-tracked cycles must not gain extra files"
        );
    }

    #[test]
    fn overlapping_cycles_are_deduped() {
        let mut direct = FileDependencyGraph::default();
        edge(&mut direct, "a.py", "b.py");
        edge(&mut direct, "b.py", "a.py");
        edge(&mut direct, "b.py", "c.py");
        edge(&mut direct, "c.py", "b.py");

        let cycles = vec![cycle(&["a.py", "b.py"]), cycle(&["b.py", "c.py"])];
        let result = collect_cyclic_files_with_direct_fallback(&cycles, &direct);
        assert_eq!(result, vec![pb("a.py"), pb("b.py"), pb("c.py")]);
    }
}
